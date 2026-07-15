use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

/// Doc 30 TASK-DEDUP-006: same window used for windowed candidate search
/// (TASK-DEDUP-003) — an existing open cluster is treated as "the same
/// ambiguous situation" if its members fall within this many days of the
/// new observation's event_time.
const CLUSTER_OVERLAP_WINDOW_DAYS: i64 = 3;

/// Doc 30 TASK-DEDUP-006: above this top score, an ambiguous case is
/// classified as `multiple_high_score_candidates` (several strong
/// candidates competing); at or below it, `mid_range_score` (candidates
/// that cleared the viability floor but without a standout).
const HIGH_SCORE_CLUSTER_REASON_THRESHOLD: f64 = 0.75;

/// Creates an ambiguity cluster in `reconciliation_clusters` and links the
/// incoming observation plus all competing candidate transactions into
/// `reconciliation_cluster_members`, per Document 18 §4.6/§4.6a — or, if an
/// existing open cluster already covers the same instrument + amount +
/// direction + overlapping date window (Doc 30 TASK-DEDUP-006), appends the
/// new observation to that cluster instead of creating a duplicate one.
///
/// Ambiguity triggers (Doc 11 §6):
///  - Multiple exact matches for the same observation
///  - Top two scored candidates within 15% margin of each other
///  - Conflicting identifiers that cannot be resolved confidently
///
/// Cluster behavior (Doc 11 §6):
///  - Stored in reconciliation_clusters, cluster_status = 'open'
///  - The incoming observation is its own member row (member_role = 'incoming')
///  - Each competing candidate transaction is a member row
///    (member_role = 'candidate_a' | 'candidate_b' | 'candidate_other')
///  - Excluded from analytics totals while cluster_status = 'open'
///  - Visible in the reconciliation console
///
/// **Hard invariant (Doc 30 TASK-DEDUP-006):** no `transactions` row is ever
/// created or modified as a side effect of this function — clusters exist
/// entirely outside the canonical analytics path until explicitly resolved.
#[allow(clippy::too_many_arguments)]
pub fn create_ambiguity_cluster(
    conn: &Connection,
    observation_id: &str,
    instrument_id: &str,
    amount_minor: i64,
    direction: &str,
    event_time: &str,
    top_score: f64,
    competing_candidate_ids: &[String],
) -> Result<String> {
    if let Some(existing_cluster_id) = find_overlapping_open_cluster(
        conn,
        instrument_id,
        amount_minor,
        direction,
        event_time,
    )? {
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, observation_id, member_role, added_at)
             VALUES (?1, ?2, ?3, 'incoming', CURRENT_TIMESTAMP)",
            params![Uuid::new_v4().to_string(), existing_cluster_id, observation_id],
        )?;
        return Ok(existing_cluster_id);
    }

    let cluster_id = Uuid::new_v4().to_string();
    let reason = if top_score > HIGH_SCORE_CLUSTER_REASON_THRESHOLD {
        "multiple_high_score_candidates"
    } else {
        "mid_range_score"
    };

    conn.execute(
        "INSERT INTO reconciliation_clusters (id, cluster_status, reason, created_at)
         VALUES (?1, 'open', ?2, CURRENT_TIMESTAMP)",
        params![cluster_id, reason],
    )?;

    conn.execute(
        "INSERT INTO reconciliation_cluster_members (id, cluster_id, observation_id, member_role, added_at)
         VALUES (?1, ?2, ?3, 'incoming', CURRENT_TIMESTAMP)",
        params![Uuid::new_v4().to_string(), cluster_id, observation_id],
    )?;

    let candidate_roles = ["candidate_a", "candidate_b", "candidate_other"];
    for (i, candidate_id) in competing_candidate_ids.iter().enumerate() {
        let member_role = candidate_roles.get(i).copied().unwrap_or("candidate_other");
        let member_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id, member_role, added_at)
             VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)",
            params![member_id, cluster_id, candidate_id, member_role],
        )?;
    }

    Ok(cluster_id)
}

/// Doc 30 TASK-DEDUP-006: "check for an existing cluster covering the same
/// instrument + amount + direction + overlapping date window; if found,
/// append the observation as a new member rather than creating a duplicate
/// cluster." Looks at each open cluster's members (whether backed by a
/// canonical transaction or a raw observation) for one matching all four
/// conditions.
fn find_overlapping_open_cluster(
    conn: &Connection,
    instrument_id: &str,
    amount_minor: i64,
    direction: &str,
    event_time: &str,
) -> Result<Option<String>> {
    let window_seconds = CLUSTER_OVERLAP_WINDOW_DAYS * 24 * 60 * 60;
    let cluster_id: Option<String> = conn
        .query_row(
            "SELECT DISTINCT c.id
             FROM reconciliation_clusters c
             JOIN reconciliation_cluster_members m ON m.cluster_id = c.id
             LEFT JOIN transactions t ON t.id = m.canonical_transaction_id
             LEFT JOIN transaction_observations o ON o.id = m.observation_id
             WHERE c.cluster_status = 'open'
               AND COALESCE(t.instrument_id, o.instrument_id) = ?1
               AND COALESCE(t.amount_minor, o.amount_minor) = ?2
               AND COALESCE(t.direction, o.direction) = ?3
               AND ABS(strftime('%s', COALESCE(t.best_event_time, o.event_time)) - strftime('%s', ?4)) <= ?5
             LIMIT 1",
            params![instrument_id, amount_minor, direction, event_time, window_seconds],
            |row| row.get(0),
        )
        .optional()?;
    Ok(cluster_id)
}

/// Resolves an existing cluster based on a user decision from the reconciliation console.
/// Supported actions (Doc 19 §10.3's documented "Allowed actions"), mapped onto
/// Document 18 §4.6's 4-value `cluster_status` enum (open/resolved/deferred/rejected):
///  - "confirm_match"    — link observation to the chosen canonical transaction; cluster_status = 'resolved'
///  - "reject_candidate" — reject the candidate relationship entirely; cluster_status = 'rejected'
///  - "keep_separate"    — keep observation and candidate as separate canonical transactions; cluster_status = 'resolved'
///  - "mark_unresolved"  — the user isn't ready to decide yet; cluster_status = 'deferred'.
///    Unlike the three actions above, this doesn't set resolved_at and the
///    cluster remains visible in `reconciliation_clusters_list`, which
///    filters on `cluster_status IN ('open', 'deferred')`.
///
/// All resolution outcomes (except `mark_unresolved`) write a match_decisions
/// row with decision = 'manually_confirmed'.
///
/// `action` is validated against this exact allowlist. Any unrecognized
/// string (a typo, a stale frontend build, or arbitrary IPC input) is
/// rejected outright rather than silently falling through to a no-op match
/// arm while the cluster is still marked resolved.
pub fn resolve_cluster(
    conn: &Connection,
    cluster_id: &str,
    observation_id: &str,
    action: &str, // "confirm_match" | "reject_candidate" | "keep_separate" | "mark_unresolved"
    chosen_canonical_id: Option<&str>,
) -> Result<()> {
    if !matches!(
        action,
        "confirm_match" | "reject_candidate" | "keep_separate" | "mark_unresolved"
    ) {
        anyhow::bail!("Unknown cluster resolution action: '{}'", action);
    }

    if action == "mark_unresolved" {
        conn.execute(
            "UPDATE reconciliation_clusters SET cluster_status = 'deferred' WHERE id = ?1",
            params![cluster_id],
        )?;

        let audit_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO audit_log (id, actor_type, actor_id, action, resource_type, resource_id, created_at)
             VALUES (?1, 'user', 'user_id', 'cluster_deferred', 'reconciliation_cluster', ?2, CURRENT_TIMESTAMP)",
            params![audit_id, cluster_id],
        )?;

        return Ok(());
    }

    let new_status = if action == "reject_candidate" {
        "rejected"
    } else {
        "resolved"
    };
    conn.execute(
        "UPDATE reconciliation_clusters SET cluster_status = ?2, resolved_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![cluster_id, new_status],
    )?;

    match action {
        "confirm_match" => {
            if let Some(canonical_id) = chosen_canonical_id {
                crate::db::transaction_observations::update_canonical_transaction_id(
                    conn,
                    observation_id,
                    Some(canonical_id),
                )?;
            }
        }
        "keep_separate" => {
            if let Some(obs_row) =
                crate::db::transaction_observations::get_observation(conn, observation_id)?
            {
                let incoming_obs = crate::reconciliation::engine::IncomingObservation {
                    id: obs_row.id,
                    instrument_id: obs_row.instrument_id.unwrap_or_default(),
                    amount_minor: obs_row.amount_minor.unwrap_or(0),
                    currency: obs_row.currency.unwrap_or_default(),
                    direction: obs_row.direction.unwrap_or_default(),
                    event_time: obs_row
                        .event_time
                        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default(),
                    reference_id: obs_row.reference_id,
                    merchant_raw: obs_row.merchant_raw,
                    source_pipeline: obs_row.source_pipeline.unwrap_or_default(),
                    source_record_id: obs_row.source_record_id.unwrap_or_default(),
                    emi_total_installments: obs_row.emi_total_installments,
                    emi_original_amount_minor: obs_row.emi_original_amount_minor,
                    fingerprint: None,                };
                crate::reconciliation::canonical::create_canonical_transaction(
                    conn,
                    &incoming_obs,
                )?;
            }
        }
        // "reject_candidate" — deliberately no canonical-transaction side
        // effect (Doc 19 §10.3): the observation is left unmatched.
        _ => {}
    }

    crate::reconciliation::audit::append_match_decision(
        conn,
        observation_id,
        chosen_canonical_id,
        1.0,
        crate::reconciliation::audit::DecisionType::ManuallyConfirmed,
    )?;

    let audit_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO audit_log (id, actor_type, actor_id, action, resource_type, resource_id, created_at)
         VALUES (?1, 'user', 'user_id', 'cluster_resolved', 'reconciliation_cluster', ?2, CURRENT_TIMESTAMP)",
        params![audit_id, cluster_id],
    )?;

    Ok(())
}

#[cfg(test)]
mod cluster_creation_tests {
    //! Doc 30 TASK-DEDUP-006: Implement Reconciliation Cluster Creation and
    //! Membership Management.
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, instrument_id, amount_minor, direction, event_time) \
             VALUES ('obs_new', 'gmail_transaction', 'msg_new', 'inst_1', 1000, 'debit', '2026-06-10 12:00:00')",
            [],
        )
        .unwrap();
        conn
    }

    /// Doc 30 TASK-DEDUP-006 acceptance test.
    #[test]
    fn test_new_cluster_created_for_first_ambiguous_case() {
        let conn = setup_test_db();

        let cluster_id = create_ambiguity_cluster(
            &conn,
            "obs_new",
            "inst_1",
            1000,
            "debit",
            "2026-06-10 12:00:00",
            0.6,
            &["cand_1".to_string()],
        )
        .unwrap();

        let (status, reason): (String, String) = conn
            .query_row(
                "SELECT cluster_status, reason FROM reconciliation_clusters WHERE id = ?1",
                params![cluster_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "open");
        assert_eq!(reason, "mid_range_score");

        let member_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reconciliation_cluster_members WHERE cluster_id = ?1",
                params![cluster_id],
                |r| r.get(0),
            )
            .unwrap();
        // 1 incoming (the observation) + 1 candidate.
        assert_eq!(member_count, 2);
    }

    /// Doc 30 TASK-DEDUP-006 acceptance test: a second ambiguous observation
    /// covering the same instrument+amount+direction+overlapping window
    /// extends the existing open cluster instead of creating a duplicate.
    #[test]
    fn test_existing_cluster_extended_for_overlapping_case() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, instrument_id, amount_minor, direction, event_time) \
             VALUES ('obs_second', 'gmail_transaction', 'msg_second', 'inst_1', 1000, 'debit', '2026-06-11 09:00:00')",
            [],
        )
        .unwrap();

        let first_cluster_id = create_ambiguity_cluster(
            &conn,
            "obs_new",
            "inst_1",
            1000,
            "debit",
            "2026-06-10 12:00:00",
            0.6,
            &["cand_1".to_string()],
        )
        .unwrap();

        // Second observation: same instrument/amount/direction, event_time
        // ~21 hours later -- well within the 3-day overlap window.
        let second_cluster_id = create_ambiguity_cluster(
            &conn,
            "obs_second",
            "inst_1",
            1000,
            "debit",
            "2026-06-11 09:00:00",
            0.6,
            &["cand_1".to_string()],
        )
        .unwrap();

        assert_eq!(first_cluster_id, second_cluster_id, "must extend the existing cluster, not create a duplicate");

        let cluster_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM reconciliation_clusters", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cluster_count, 1);

        let member_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reconciliation_cluster_members WHERE cluster_id = ?1",
                params![first_cluster_id],
                |r| r.get(0),
            )
            .unwrap();
        // 2 incoming (obs_new, obs_second) + 1 candidate from the first call
        // (the second call's candidate list is not re-added since the
        // cluster already exists).
        assert_eq!(member_count, 3);
    }

    /// Doc 30 TASK-DEDUP-006 acceptance test: cluster creation must never
    /// create or modify a `transactions` row as a side effect.
    #[test]
    fn test_cluster_creation_does_not_touch_transactions_table() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, is_deleted) \
             VALUES ('cand_1', 'inst_1', 1000, 'USD', 'debit', 0)",
            [],
        )
        .unwrap();

        let before_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        let before_snapshot: String = conn
            .query_row(
                "SELECT merchant_display_name FROM transactions WHERE id = 'cand_1'",
                [],
                |r| r.get::<_, Option<String>>(0).map(|v| v.unwrap_or_default()),
            )
            .unwrap();

        create_ambiguity_cluster(
            &conn,
            "obs_new",
            "inst_1",
            1000,
            "debit",
            "2026-06-10 12:00:00",
            0.9,
            &["cand_1".to_string()],
        )
        .unwrap();

        let after_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        let after_snapshot: String = conn
            .query_row(
                "SELECT merchant_display_name FROM transactions WHERE id = 'cand_1'",
                [],
                |r| r.get::<_, Option<String>>(0).map(|v| v.unwrap_or_default()),
            )
            .unwrap();

        assert_eq!(before_count, after_count);
        assert_eq!(before_snapshot, after_snapshot);
    }

    /// A non-overlapping (different instrument) ambiguous case must not be
    /// folded into an unrelated existing cluster.
    #[test]
    fn test_non_overlapping_case_creates_separate_cluster() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, instrument_id, amount_minor, direction, event_time) \
             VALUES ('obs_other_inst', 'gmail_transaction', 'msg_other', 'inst_2', 1000, 'debit', '2026-06-10 12:00:00')",
            [],
        )
        .unwrap();

        let first_cluster_id = create_ambiguity_cluster(
            &conn,
            "obs_new",
            "inst_1",
            1000,
            "debit",
            "2026-06-10 12:00:00",
            0.6,
            &["cand_1".to_string()],
        )
        .unwrap();

        let second_cluster_id = create_ambiguity_cluster(
            &conn,
            "obs_other_inst",
            "inst_2",
            1000,
            "debit",
            "2026-06-10 12:00:00",
            0.6,
            &["cand_2".to_string()],
        )
        .unwrap();

        assert_ne!(first_cluster_id, second_cluster_id);
    }
}
