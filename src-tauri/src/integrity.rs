//! Binary integrity self-check at launch (Doc 26 T-10, I11 fix).
//!
//! macOS's `SecStaticCodeCheckValidity` API (what `codesign --verify` calls
//! under the hood) confirms the running executable's code signature is
//! intact and matches its on-disk bytes — a documented, previously
//! unimplemented anti-tampering / anti-dylib-injection control. Only
//! meaningful for signed release builds: local dev builds are typically
//! unsigned or ad-hoc signed, so this is a no-op outside release builds.

use std::process::Command;

/// Verifies the currently-running executable's code signature via
/// `codesign --verify --strict`, matching the semantics of
/// `SecStaticCodeCheckValidity`. Returns `Ok(())` if the signature is valid,
/// `Err` with `codesign`'s diagnostic otherwise (invalid/missing signature,
/// or the binary's bytes no longer match what was signed).
#[cfg(target_os = "macos")]
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

#[cfg(not(target_os = "macos"))]
pub fn verify_binary_integrity() -> anyhow::Result<()> {
    // Doc 26 T-10 is scoped to the macOS distribution — no-op elsewhere.
    Ok(())
}
