//! Verifies that the running binary has not been tampered with.
//!
//! On macOS this shells out to `codesign`, which checks the application's
//! signature and the seal over its bundled resources. `--deep` extends the check
//! to nested code such as the sidecar executable, so a swapped-in binary inside
//! the bundle is caught rather than only a modified main executable.
//!
//! Every other platform gets a no-op implementation, so callers need no
//! conditional compilation of their own.

use std::process::Command;

#[cfg(target_os = "macos")]
/// Verifies the macOS code signature over the bundle and its resources.
pub fn verify_binary_integrity() -> anyhow::Result<()> {
    let exe_path = std::env::current_exe()?;

    let output = Command::new("codesign")
        .args(["--verify", "--strict", "--deep"])
        .arg(&exe_path)
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!(
            "codesign verification failed for {}: {}",
            exe_path.display(),
            stderr.trim()
        ))
    }
}

// No signature scheme wired up off macOS; succeed rather than block startup.
#[cfg(not(target_os = "macos"))]
/// No-op on platforms with no signature scheme wired up.
pub fn verify_binary_integrity() -> anyhow::Result<()> {
    Ok(())
}
