//! Derives the stable device identifier used for licence binding.
//!
//! Must be stable across restarts and OS updates, or a legitimate user would be
//! repeatedly told their licence is bound elsewhere.
use anyhow::Result;
use uuid::Uuid;

const DEVICE_ID_NAMESPACE: Uuid = Uuid::from_bytes([
    0xd1, 0x7e, 0x70, 0x00, 0x11, 0x11, 0x51, 0x11, 0x91, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
]);

/// Returns the stable device id used for licence binding.
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
