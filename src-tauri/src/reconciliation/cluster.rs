use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Creates an ambiguity cluster in `reconciliation_clusters` and links the
/// incoming observation plus all competing candidate transactions into
/// `reconciliation_cluster_members`, per Document 18 §4.6/§4.6a.
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
pub fn create_ambiguity_cluster(
    conn: &Connection,
    observation_id: &str,
    competing_candidate_ids: &[String],
) -> Result<String> {
    let cluster_id = Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO reconciliation_clusters (id, cluster_status, created_at)
         VALUES (?1, 'open', CURRENT_TIMESTAMP)",
        params![cluster_id],
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
                };
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
