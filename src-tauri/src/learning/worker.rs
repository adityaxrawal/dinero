use deadpool_sqlite::Pool;
use tokio::sync::mpsc;

/// Bounded so a runaway producer cannot grow memory without limit. Corrections
/// are human-paced, so this is never reached in practice; when it is, [`enqueue`]
/// drops rather than waits.
pub(crate) const FEEDBACK_QUEUE_CAPACITY: usize = 128;

/// How much of a bank's settled history a candidate is regressed against.
/// Twenty is enough to catch a rule that fires on the wrong template shape
/// without making the check a scan.
const REGRESSION_SAMPLE_LIMIT: usize = 20;

/// One field correction, with everything the worker needs to author a rule from
/// it. The source text is captured by the *producer*, at correction time, rather
/// than re-fetched here — the retention sweep could otherwise null the body
/// between the save and the worker picking the job up.
pub struct FeedbackJob {
    pub feedback_log_id: String,
    pub bank_name: String,
    pub field_name: String,
    /// `'email' | 'statement_pdf'`.
    pub source_type: String,
    /// The email body, or the statement row's `description_raw`.
    pub source_text: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub observation_id: Option<String>,
    /// `'user_edit' | 'drift_llm' | 'batch_cleanup'`.
    pub learned_from: String,
    pub app_dir: Option<std::path::PathBuf>,
}

#[derive(Clone)]
pub struct LearningHandle {
    pub tx: mpsc::Sender<FeedbackJob>,
}

/// Which author produced a candidate. Recorded on the row so a later "why is
/// this rule here" question is answerable — it grants no extra trust, since both
/// arms cleared the identical gate.
pub(crate) enum Authored {
    Deterministic(serde_json::Value),
    Llm(serde_json::Value),
    None,
}

/// One sequential consumer. Corrections arrive at human pace — a worker pool
/// would add contention on the LLM sidecar for no throughput anyone can
/// perceive.
///
/// ponytail: single consumer; revisit only if a bulk re-correction feature ever
/// makes this a real queue.
pub fn spawn_learning_worker(pool: Pool) -> LearningHandle {
    let (tx, mut rx) = mpsc::channel::<FeedbackJob>(FEEDBACK_QUEUE_CAPACITY);
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            process_feedback_job(job, &pool).await;
        }
    });
    LearningHandle { tx }
}

/// Hands a job to the worker without ever blocking the caller.
///
/// `try_send` rather than `send`: the caller is an IPC command that has already
/// committed the user's correction and is about to return. Waiting on a full
/// channel would make saving a transaction slow for a background nicety, and
/// failing would surface a learning-pipeline problem as a failed save. A dropped
/// job means one correction is not learned from; the correction itself is
/// already safe on disk.
pub async fn enqueue(handle: &LearningHandle, job: FeedbackJob) {
    if let Err(e) = handle.tx.try_send(job) {
        tracing::debug!("learning queue full or closed, dropping feedback job: {e}");
    }
}

/// Author a rule for one correction, validate it, store it if it holds.
///
/// Every exit is a no-op for the user's data: the correction was committed
/// before this job existed, and nothing here writes to `transactions` or
/// `transaction_observations`.
pub(crate) async fn process_feedback_job(job: FeedbackJob, pool: &Pool) {
    if job.source_text.trim().is_empty() || job.new_value.trim().is_empty() {
        // No retained source (purged past retention, or a manually-created
        // transaction) — log-only, nothing to anchor on.
        tracing::debug!(
            field = %job.field_name,
            "learning: no source text for this correction, nothing to learn from"
        );
        return;
    }

    let template_hash = crate::extraction::ladder::compute_template_hash(&job.source_text);

    // ── Author: deterministic first, LLM only if it cannot produce one ───────
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

    // ── Gate step 1: self-check ──────────────────────────────────────────────
    // Belt and braces — both authors already ran it, but this is the one
    // invariant that must never depend on a caller having remembered to.
    let needles =
        crate::extraction::rule_synthesis::needle_candidates(&job.field_name, &job.new_value);
    let is_override = payload.get("override_value").is_some();
    if !is_override
        && !crate::extraction::rule_synthesis::self_check(&payload, &job.source_text, &needles)
    {
        record_rejection(pool, &job, Some(payload), "self-check failed").await;
        return;
    }

    // ── Gate step 2: regression against this bank's settled history ──────────
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

    // ── Store ────────────────────────────────────────────────────────────────
    // A confirmed user correction (or a validated LLM rewrite of one) is ground
    // truth and goes live immediately. A drift-detected candidate is a guess and
    // still earns activation through 3 auto-successes.
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
            // An empty feedback_log_id means this candidate came from drift
            // detection, which has no user correction to point at.
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

/// The fallback author. Silent no-op when the model is unavailable — an
/// LLM-less machine simply learns from the deterministic pass alone, which is
/// where the large majority of corrections land anyway.
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
            // TimedOut / InfraFailed / Rejected all land here. The worker just
            // does not write a rule.
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

/// Same resolution `commands::merchant_cleanup` uses — the stored preference if
/// its model is actually downloaded, otherwise whatever is.
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
            // No app_dir: the LLM fallback must be unreachable in tests, so a
            // pass here proves the deterministic path did the work.
            app_dir: None,
        }
    }

    /// The headline behaviour: a correction becomes an immediately-active rule.
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

    /// Drift-detected guesses do not get day-one trust.
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

    /// The edge case the design calls out: nothing written, nothing broken.
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

    /// Scoping is structural: rules never cross banks or source types.
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

    /// The channel must never make a correction slow or fail it.
    #[tokio::test]
    async fn enqueueing_never_blocks_the_caller() {
        let (tx, _rx) = mpsc::channel::<FeedbackJob>(FEEDBACK_QUEUE_CAPACITY);
        let handle = LearningHandle { tx };
        // Far more than the channel capacity, with no consumer draining it; a
        // full channel must drop rather than await, because the caller is an
        // IPC command mid-save.
        for _ in 0..(FEEDBACK_QUEUE_CAPACITY * 2) {
            enqueue(&handle, job("merchant", "RAZ*SWIGGY LIMITE BANGALORE")).await;
        }
    }
}
