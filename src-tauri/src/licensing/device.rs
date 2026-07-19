use anyhow::Result;
use uuid::Uuid;

/// Fixed namespace UUID used to derive the stable licensing `device_id` via
/// UUID-v5 (Doc 22 §11.1). Not a secret — just a stable salt so the same
/// hardware UUID always hashes to the same `device_id`.
const DEVICE_ID_NAMESPACE: Uuid = Uuid::from_bytes([
    0xd1, 0x7e, 0x70, 0x00, 0x11, 0x11, 0x51, 0x11, 0x91, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
]);

/// Derives the stable licensing `device_id` (Doc 22 §11.1): reads the Mac's
/// hardware UUID (`IOPlatformUUID`) via the `machine-uid` crate, then hashes it
/// to a UUID-v5. Stable across app reinstalls on the same Mac, generated
/// entirely locally with no network call, and never mixed with Gmail account
/// data or financial data — this value exists solely to bind a license JWT to
/// the machine it was issued for (§11.2).
pub fn get_device_id() -> Result<String> {
    let hw_uuid =
        machine_uid::get().map_err(|e| anyhow::anyhow!("Failed to read hardware UUID: {}", e))?;
    let device_uuid = Uuid::new_v5(&DEVICE_ID_NAMESPACE, hw_uuid.as_bytes());
    Ok(device_uuid.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id_is_stable_across_calls() {
        let first = get_device_id().expect("device_id derivation must succeed");
        let second = get_device_id().expect("device_id derivation must succeed");
        assert_eq!(
            first, second,
            "device_id must be stable across calls on the same machine"
        );
    }

    #[test]
    fn test_device_id_is_a_valid_uuid() {
        let id = get_device_id().expect("device_id derivation must succeed");
        assert!(
            Uuid::parse_str(&id).is_ok(),
            "device_id must be a valid UUID string"
        );
    }
}
