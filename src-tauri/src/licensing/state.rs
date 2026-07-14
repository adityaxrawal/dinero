use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result as SqliteResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LicenseStatus {
    AnonymousEval,
    Trial,
    Active,
    Grace,
    Locked,
    PastDue,
    Canceled,
    Expired,
}

impl LicenseStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LicenseStatus::AnonymousEval => "anonymous",
            LicenseStatus::Trial => "trialing",
            LicenseStatus::Active => "active",
            LicenseStatus::Grace => "grace",
            LicenseStatus::Locked => "locked",
            LicenseStatus::PastDue => "past_due",
            LicenseStatus::Canceled => "canceled",
            LicenseStatus::Expired => "expired",
        }
    }

    pub fn parse_status(s: &str) -> Option<Self> {
        match s {
            "anonymous" => Some(LicenseStatus::AnonymousEval),
            "trialing" => Some(LicenseStatus::Trial),
            "active" => Some(LicenseStatus::Active),
            "grace" => Some(LicenseStatus::Grace),
            "locked" => Some(LicenseStatus::Locked),
            "past_due" => Some(LicenseStatus::PastDue),
            "canceled" => Some(LicenseStatus::Canceled),
            "expired" => Some(LicenseStatus::Expired),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseStateRow {
    pub id: i64,
    pub license_jwt: String,
    pub subscription_status_cached: LicenseStatus,
    pub plan_id_cached: Option<String>,
    pub current_period_end_cached: Option<DateTime<Utc>>,
    pub jwt_expires_at: DateTime<Utc>,
    pub last_server_validated_at: Option<DateTime<Utc>>,
    pub last_known_valid_time: DateTime<Utc>,
    pub device_fingerprint: Option<String>,
    pub source: String,
    pub billing_interval_cached: Option<String>,
}

pub fn get_license_state(conn: &Connection) -> SqliteResult<Option<LicenseStateRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, license_jwt, subscription_status_cached, plan_id_cached,
                current_period_end_cached, jwt_expires_at, last_server_validated_at,
                last_known_valid_time, device_fingerprint, source, billing_interval_cached
         FROM license_state WHERE id = 1"
    )?;

    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let status_str: String = row.get(2)?;
        let status = LicenseStatus::parse_status(&status_str).unwrap_or(LicenseStatus::Locked);

        Ok(Some(LicenseStateRow {
            id: row.get(0)?,
            license_jwt: row.get(1)?,
            subscription_status_cached: status,
            plan_id_cached: row.get(3)?,
            current_period_end_cached: row.get(4)?,
            jwt_expires_at: row.get(5)?,
            last_server_validated_at: row.get(6)?,
            last_known_valid_time: row.get(7)?,
            device_fingerprint: row.get(8)?,
            source: row.get(9)?,
            billing_interval_cached: row.get(10)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn upsert_license_state(conn: &Connection, state: &LicenseStateRow) -> SqliteResult<()> {
    conn.execute(
        "INSERT INTO license_state (
            id, license_jwt, subscription_status_cached, plan_id_cached,
            current_period_end_cached, jwt_expires_at, last_server_validated_at,
            last_known_valid_time, device_fingerprint, source, billing_interval_cached
        ) VALUES (
            1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
        )
        ON CONFLICT(id) DO UPDATE SET
            license_jwt = excluded.license_jwt,
            subscription_status_cached = excluded.subscription_status_cached,
            plan_id_cached = excluded.plan_id_cached,
            current_period_end_cached = excluded.current_period_end_cached,
            jwt_expires_at = excluded.jwt_expires_at,
            last_server_validated_at = excluded.last_server_validated_at,
            last_known_valid_time = excluded.last_known_valid_time,
            device_fingerprint = excluded.device_fingerprint,
            source = excluded.source,
            billing_interval_cached = excluded.billing_interval_cached,
            updated_at = CURRENT_TIMESTAMP",
        params![
            state.license_jwt,
            state.subscription_status_cached.as_str(),
            state.plan_id_cached,
            state.current_period_end_cached,
            state.jwt_expires_at,
            state.last_server_validated_at,
            state.last_known_valid_time,
            state.device_fingerprint,
            state.source,
            state.billing_interval_cached,
        ],
    )?;
    Ok(())
}

/// TASK-AUTH-009: routes through `state_machine::transition` so this can
/// never silently move an already-`Locked` (or any other illegal source
/// state) row — previously this ran an unconditional raw `UPDATE` with no
/// legality check at all.
pub fn transition_to_locked(conn: &Connection, reason_clock_skew: bool) -> anyhow::Result<()> {
    super::state_machine::transition(conn, LicenseStatus::Locked)?;
    if reason_clock_skew {
        tracing::warn!("ClockSkewDetected: Transitioned license to LOCKED state.");
    }
    Ok(())
}

pub fn record_known_valid_time(conn: &Connection, time: DateTime<Utc>) -> SqliteResult<()> {
    conn.execute(
        "UPDATE license_state SET last_known_valid_time = ?1 WHERE id = 1",
        params![time],
    )?;
    Ok(())
}
