use crate::db::sender_reputation::*;
use crate::db::test_helpers::setup_test_db;

#[test]
fn test_first_sighting_has_no_prior_history() {
    let conn = setup_test_db();
    assert!(!has_prior_sighting(&conn, "newbank.example").unwrap());

    record_sighting(&conn, "newbank.example", "verified_transaction_candidate").unwrap();

    // The message that was just recorded is itself the first sighting --
    // a second, later message from the same domain is the one that should
    // see history.
    assert!(has_prior_sighting(&conn, "newbank.example").unwrap());
}

#[test]
fn test_record_sighting_increments_counts_and_tracks_pass_rate() {
    let conn = setup_test_db();

    record_sighting(&conn, "hdfcbank.net", "verified_transaction_candidate").unwrap();
    record_sighting(&conn, "hdfcbank.net", "verified_transaction_candidate").unwrap();
    record_sighting(&conn, "hdfcbank.net", "spoof_reject").unwrap();

    let rep = get_reputation(&conn, "hdfcbank.net").unwrap().unwrap();
    assert_eq!(rep.message_count, 3);
    assert_eq!(rep.verified_pass_count, 2);
    assert_eq!(rep.last_verification_result, "spoof_reject");
}

#[test]
fn test_get_reputation_none_for_unknown_domain() {
    let conn = setup_test_db();
    assert!(get_reputation(&conn, "never-seen.example")
        .unwrap()
        .is_none());
}

#[test]
fn test_pending_sender_repeat_rejection_increments_reject_count_not_duplicate_rows() {
    let conn = setup_test_db();

    record_rejection_candidate(
        &conn,
        "id1",
        "newfintech.example",
        "New Fintech",
        "transaction_candidate",
    )
    .unwrap();
    record_rejection_candidate(
        &conn,
        "id2",
        "newfintech.example",
        "New Fintech",
        "transaction_candidate",
    )
    .unwrap();

    let pending = select_pending(&conn).unwrap();
    assert_eq!(
        pending.len(),
        1,
        "repeat rejection of the same domain must not duplicate rows"
    );
    assert_eq!(pending[0].reject_count, 2);
}

#[test]
fn test_approved_pending_sender_appears_in_approved_domains() {
    let conn = setup_test_db();
    record_rejection_candidate(
        &conn,
        "id1",
        "newfintech.example",
        "New Fintech",
        "transaction_candidate",
    )
    .unwrap();

    assert!(select_approved_domains(&conn).unwrap().is_empty());

    update_status(&conn, "id1", "approved").unwrap();

    let approved = select_approved_domains(&conn).unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].domain, "newfintech.example");
}

#[test]
fn test_reject_after_approval_does_not_reset_status() {
    let conn = setup_test_db();
    record_rejection_candidate(
        &conn,
        "id1",
        "newfintech.example",
        "New Fintech",
        "transaction_candidate",
    )
    .unwrap();
    update_status(&conn, "id1", "approved").unwrap();

    // A later message from the same domain still fails string-based
    // verification before the approved-domains layer is consulted upstream
    // -- record_rejection_candidate must not flip an already-approved
    // domain back towards pending.
    record_rejection_candidate(
        &conn,
        "id1",
        "newfintech.example",
        "New Fintech",
        "transaction_candidate",
    )
    .unwrap();

    let approved = select_approved_domains(&conn).unwrap();
    assert_eq!(approved.len(), 1, "approved domain must remain approved");
}

#[test]
fn test_invalid_status_transition_rejected() {
    let conn = setup_test_db();
    record_rejection_candidate(
        &conn,
        "id1",
        "newfintech.example",
        "New Fintech",
        "transaction_candidate",
    )
    .unwrap();
    assert!(update_status(&conn, "id1", "pending").is_err());
}
