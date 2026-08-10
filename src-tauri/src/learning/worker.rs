//! Background worker that processes correction feedback into rules.
//!
//! Queued and asynchronous by design: rule synthesis can invoke the LLM, and
//! making the user wait on that to edit a merchant name would be intolerable.
use deadpool_sqlite::Pool;
use tokio::sync::mpsc;

pub(crate) const FEEDBACK_QUEUE_CAPACITY: usize = 128;

const REGRESSION_SAMPLE_LIMIT: usize = 20;

pub struct FeedbackJob {
    pub feedback_log_id: String,
    pub bank_name: String,
    pub field_name: String,
    pub source_type: String,
    pub source_text: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub observation_id: Option<String>,
    pub learned_from: String,
    pub app_dir: Option<std::path::PathBuf>,
}

#[derive(Clone)]
pub struct LearningHandle {
    pub tx: mpsc::Sender<FeedbackJob>,
}

pub(crate) enum Authored {
    Deterministic(serde_json::Value),
    Llm(serde_json::Value),
    None,
}

/// ponytail: single consumer; revisit only if a bulk re-correction feature ever
pub fn spawn_learning_worker(pool: Pool) -> LearningHandle {
    let (tx, mut rx) = mpsc::channel::<FeedbackJob>(FEEDBACK_QUEUE_CAPACITY);
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            process_feedback_job(job, &pool).await;
        }
    });
    LearningHandle { tx }
}

/// Queues a feedback job for background processing.
pub async fn enqueue(handle: &LearningHandle, job: FeedbackJob) {
    if let Err(e) = handle.tx.try_send(job) {
        tracing::debug!("learning queue full or closed, dropping feedback job: {e}");
    }
}

/// Turns one user correction into a synthesised rule.
///
/// Runs on the worker rather than inline, because synthesis can invoke the LLM
/// and the user's edit must stay instant.
pub(crate) async fn process_feedback_job(job: FeedbackJob, pool: &Pool) {
    if job.source_text.trim().is_empty() || job.new_value.trim().is_empty() {
        tracing::debug!(
            field = %job.field_name,
            "learning: no source text for this correction, nothing to learn from"
        );
        return;
    }

    let template_hash = crate::extraction::ladder::compute_template_hash(&job.source_text);

    let authored = match crate::extraction::rule_synthesis::synthesize(
        &job.field_name,
        &job.source_text,
        &job.new_value,
    ) {
        Some(payload) => Authored::Deterministic(payload),
        None => author_with_llm(&job, pool).await,
    };

    let (payload, authored_by) = match authored {
        Authored::Deterministic(p) => (p, "deterministic"),
        Authored::Llm(p) => (p, "llm"),
        Authored::None => {
            record_rejection(
                pool,
                &job,
                None,
                "no self-consistent candidate from either author",
            )
            .await;
            return;
        }
    };

    let needles =
        crate::extraction::rule_synthesis::needle_candidates(&job.field_name, &job.new_value);
    let is_override = payload.get("override_value").is_some();
    if !is_override
        && !crate::extraction::rule_synthesis::self_check(&payload, &job.source_text, &needles)
    {
        record_rejection(pool, &job, Some(payload), "self-check failed").await;
        return;
    }

    let samples = {
        let (bank, field, source_type, obs) = (
            job.bank_name.clone(),
            job.field_name.clone(),
            job.source_type.clone(),
            job.observation_id.clone(),
        );
        match pool.get().await {
            Ok(conn) => conn
                .interact(move |c| {
                    crate::db::field_rules::historical_samples(
                        c,
                        &bank,
                        &field,
                        &source_type,
                        obs.as_deref(),
                        REGRESSION_SAMPLE_LIMIT,
                    )
                })
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    };

    if let Err(reason) =
        crate::extraction::rule_synthesis::regression_check(&payload, &samples, &job.field_name)
    {
        record_rejection(pool, &job, Some(payload), &reason).await;
        return;
    }

    let status = if job.learned_from == "drift_llm" {
        "pending"
    } else {
        "active"
    };

    let now = chrono::Utc::now().naive_utc();
    let variant = crate::db::field_rules::FieldRuleVariant {
        id: uuid::Uuid::new_v4().to_string(),
        bank_name: job.bank_name.clone(),
        field_name: job.field_name.clone(),
        source_type: job.source_type.clone(),
        template_hash,
        rule_payload_json: payload,
        status: status.to_string(),
        success_count: if status == "active" { 1 } else { 0 },
        failure_count: 0,
        confidence: if status == "active" { 1.0 } else { 0.0 },
        authored_by: authored_by.to_string(),
        learned_from: job.learned_from.clone(),
        created_at: Some(now),
        updated_at: Some(now),
    };

    let feedback_id = job.feedback_log_id.clone();
    let field_for_log = job.field_name.clone();
    let bank_for_log = job.bank_name.clone();
    let Ok(conn) = pool.get().await else { return };
    let stored = conn
        .interact(move |c| {
            let feedback_ref = if feedback_id.is_empty() {
                None
            } else {
                Some(feedback_id.as_str())
            };
            crate::db::field_rules::upsert_variant(c, &variant, feedback_ref)
        })
        .await;

    match stored {
        Ok(Ok(id)) => tracing::info!(
            bank = %bank_for_log,
            field = %field_for_log,
            rule_id = %id,
            authored_by,
            status,
            "learning: stored a validated extraction rule"
        ),
        Ok(Err(e)) => tracing::warn!("learning: failed to store rule: {e}"),
        Err(e) => tracing::warn!("learning: pool interact failed while storing rule: {e:?}"),
    }
}

/// Authors a replacement rule using the LLM.
async fn author_with_llm(job: &FeedbackJob, pool: &Pool) -> Authored {
    let Some(app_dir) = job.app_dir.as_deref() else {
        return Authored::None;
    };
    let Some(model_id) = resolve_model(pool, app_dir).await else {
        return Authored::None;
    };

    let existing: Vec<String> = {
        let (bank, source_type) = (job.bank_name.clone(), job.source_type.clone());
        match pool.get().await {
            Ok(conn) => conn
                .interact(move |c| {
                    crate::db::field_rules::select_live_by_bank(c, &bank, &source_type)
                })
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|r| {
                    r.rule_payload_json
                        .get("regex")?
                        .as_str()
                        .map(|s| format!("{}: {s}", r.field_name))
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    };

    let prompt = crate::extraction::rule_llm::generate_prompt(
        &job.field_name,
        &job.bank_name,
        &job.source_text,
        job.old_value.as_deref(),
        &job.new_value,
        &existing,
    );

    let raw = match crate::llama_sidecar::complete_with_schema_and_context(
        app_dir,
        &model_id,
        &prompt,
        crate::extraction::rule_llm::authoring_schema(),
        crate::logging::llm_logger::LlmCallContext::new(
            crate::logging::llm_logger::LlmCallType::RuleAuthoring,
            1,
        ),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(field = %job.field_name, "learning: LLM authoring failed: {e}");
            return Authored::None;
        }
    };

    match crate::extraction::rule_llm::validate(
        &raw,
        &job.field_name,
        &job.source_text,
        &job.new_value,
    ) {
        Some(payload) => Authored::Llm(payload),
        None => Authored::None,
    }
}

/// Resolves which model to use, or None if unavailable.
async fn resolve_model(pool: &Pool, app_dir: &std::path::Path) -> Option<String> {
    let stored = match pool.get().await {
        Ok(conn) => conn
            .interact(|c| crate::db::local_profile::get_llm_model(c))
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten(),
        Err(_) => None,
    };
    let downloaded: Vec<String> = crate::llm_manager::get_available_models()
        .into_iter()
        .filter(|m| crate::llm_manager::get_model_path(app_dir, &m.id).is_some())
        .map(|m| m.id)
        .collect();
    crate::llm_manager::resolve_active_model(&downloaded, stored.as_deref())
}

/// Records a rejected rule candidate, so it is not re-derived repeatedly.
async fn record_rejection(
    pool: &Pool,
    job: &FeedbackJob,
    payload: Option<serde_json::Value>,
    reason: &str,
) {
    tracing::debug!(
        bank = %job.bank_name,
        field = %job.field_name,
        reason,
        "learning: candidate rejected, extraction left unchanged"
    );
    let (feedback_id, reason) = (job.feedback_log_id.clone(), reason.to_string());
    if let Ok(conn) = pool.get().await {
        let _ = conn
            .interact(move |c| {
                let feedback_ref = if feedback_id.is_empty() {
                    None
                } else {
                    Some(feedback_id.as_str())
                };
                crate::db::field_rules::log_rejection(c, feedback_ref, payload.as_ref(), &reason)
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "Dear Cardholder, Rs.245.43 spent on your SBI Credit Card \
                        ending 7603 at RAZ*SWIGGY LIMITE BANGALORE on 01/07/26.";

    async fn setup_pool() -> Pool {
        let path = crate::db::test_helpers::fresh_temp_db_path();
        crate::db::migrations::run_migrations(&path, None)
            .await
            .unwrap();
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config {
                path: path.clone(),
                pool: Some(deadpool_sqlite::PoolConfig::new(2)),
            },
            deadpool_sqlite::Runtime::Tokio1,
        );
        Pool::builder(mgr).build().unwrap()
    }

    fn job(field: &str, new_value: &str) -> FeedbackJob {
        FeedbackJob {
            feedback_log_id: "fb_1".to_string(),
            bank_name: "SBI".to_string(),
            field_name: field.to_string(),
            source_type: "email".to_string(),
            source_text: BODY.to_string(),
            old_value: Some("WRONG".to_string()),
            new_value: new_value.to_string(),
            observation_id: Some("obs_1".to_string()),
            learned_from: "user_edit".to_string(),
            app_dir: None,
        }
    }

    #[tokio::test]
    async fn a_user_correction_becomes_an_active_rule() {
        let pool = setup_pool().await;
        process_feedback_job(job("merchant", "RAZ*SWIGGY LIMITE BANGALORE"), &pool).await;

        let conn = pool.get().await.unwrap();
        let rules = conn
            .interact(|c| crate::db::field_rules::select_live_by_bank(c, "SBI", "email"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].status, "active",
            "a confirmed correction is ground truth"
        );
        assert_eq!(rules[0].authored_by, "deterministic");
        assert_eq!(rules[0].learned_from, "user_edit");
        assert_eq!(rules[0].field_name, "merchant");
    }

    #[tokio::test]
    async fn a_drift_candidate_starts_pending() {
        let pool = setup_pool().await;
        let mut j = job("merchant", "RAZ*SWIGGY LIMITE BANGALORE");
        j.learned_from = "drift_llm".to_string();
        process_feedback_job(j, &pool).await;

        let conn = pool.get().await.unwrap();
        let all = conn
            .interact(|c| crate::db::field_rules::select_all(c))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(
            all[0].status, "pending",
            "an auto-detected guess must earn activation"
        );
    }

    #[tokio::test]
    async fn a_value_absent_from_the_source_is_rejected_not_applied() {
        let pool = setup_pool().await;
        process_feedback_job(job("merchant", "ZOMATO"), &pool).await;

        let conn = pool.get().await.unwrap();
        let (variants, rejections): (i64, i64) = conn
            .interact(|c| {
                let v = c
                    .query_row("SELECT COUNT(*) FROM field_rule_variants", [], |r| r.get(0))
                    .unwrap();
                let r = c
                    .query_row(
                        "SELECT COUNT(*) FROM rule_change_log WHERE action = 'rejected'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                (v, r)
            })
            .await
            .unwrap();
        assert_eq!(variants, 0, "a rejected candidate must write no rule");
        assert_eq!(
            rejections, 1,
            "but it must leave a trace of having been tried"
        );
    }

    #[tokio::test]
    async fn an_empty_source_is_a_no_op_not_a_crash() {
        let pool = setup_pool().await;
        let mut j = job("merchant", "SWIGGY");
        j.source_text = String::new();
        process_feedback_job(j, &pool).await;

        let conn = pool.get().await.unwrap();
        let variants: i64 = conn
            .interact(|c| c.query_row("SELECT COUNT(*) FROM field_rule_variants", [], |r| r.get(0)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(variants, 0);
    }

    #[tokio::test]
    async fn an_amount_correction_learns_from_the_printed_form() {
        let pool = setup_pool().await;
        process_feedback_job(job("amount", "24543"), &pool).await;

        let conn = pool.get().await.unwrap();
        let rules = conn
            .interact(|c| crate::db::field_rules::select_live_by_bank(c, "SBI", "email"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rules.len(), 1);
        let recovered =
            crate::extraction::rule_synthesis::apply_payload(&rules[0].rule_payload_json, BODY)
                .unwrap();
        assert_eq!(recovered.trim(), "245.43");
    }

    #[tokio::test]
    async fn a_direction_correction_writes_an_override() {
        let pool = setup_pool().await;
        process_feedback_job(job("direction", "credit"), &pool).await;

        let conn = pool.get().await.unwrap();
        let rules = conn
            .interact(|c| crate::db::field_rules::select_live_by_bank(c, "SBI", "email"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rules[0].rule_payload_json["override_value"], "credit");
    }

    #[tokio::test]
    async fn a_rule_is_scoped_to_its_bank_and_source_type() {
        let pool = setup_pool().await;
        process_feedback_job(job("merchant", "RAZ*SWIGGY LIMITE BANGALORE"), &pool).await;

        let conn = pool.get().await.unwrap();
        let (other_bank, other_source) = conn
            .interact(|c| {
                (
                    crate::db::field_rules::select_live_by_bank(c, "HDFC", "email").unwrap(),
                    crate::db::field_rules::select_live_by_bank(c, "SBI", "statement_pdf").unwrap(),
                )
            })
            .await
            .unwrap();
        assert!(other_bank.is_empty());
        assert!(other_source.is_empty());
    }

    #[tokio::test]
    async fn enqueueing_never_blocks_the_caller() {
        let (tx, _rx) = mpsc::channel::<FeedbackJob>(FEEDBACK_QUEUE_CAPACITY);
        let handle = LearningHandle { tx };
        for _ in 0..(FEEDBACK_QUEUE_CAPACITY * 2) {
            enqueue(&handle, job("merchant", "RAZ*SWIGGY LIMITE BANGALORE")).await;
        }
    }
}
