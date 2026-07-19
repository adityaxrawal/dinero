use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};

/// Structured error type for file validation failures (Doc 10 §5.1).
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("invalid_file_type: file must be application/pdf")]
    InvalidFileType,
    #[error("file_too_large: file must not exceed 5 MB")]
    FileTooLarge,
    #[error("empty_file: file must not be zero bytes")]
    EmptyFile,
}

/// Validates an incoming PDF byte buffer per Doc 10 §5.1.
///
/// Rules:
/// 1. MIME sniff — first 4 bytes must be `%PDF`
/// 2. Size ≤ 5 MB (5 * 1024 * 1024 bytes)
/// 3. Not zero-byte
///
/// Returns the SHA-256 hex digest of the file on success (§5.2).
/// The digest is used downstream for exact-duplicate detection.
pub fn validate_and_hash(bytes: &[u8]) -> Result<String> {
    // Rule 3 — empty file
    if bytes.is_empty() {
        return Err(anyhow!(ValidationError::EmptyFile));
    }

    // Rule 1 — MIME sniff: PDF magic bytes = %PDF = 0x25 0x50 0x44 0x46
    if bytes.len() < 4 || &bytes[..4] != b"%PDF" {
        return Err(anyhow!(ValidationError::InvalidFileType));
    }

    // Rule 2 — size cap: 5 MB
    const MAX_BYTES: usize = 5 * 1024 * 1024;
    if bytes.len() > MAX_BYTES {
        return Err(anyhow!(ValidationError::FileTooLarge));
    }

    // Compute SHA-256 (§5.2) — used to detect bit-identical duplicate uploads
    let digest = Sha256::digest(bytes);
    let hex = format!("{:x}", digest);

    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_file() {
        assert!(validate_and_hash(&[]).is_err());
    }

    #[test]
    fn test_non_pdf_rejected() {
        // PNG header
        let fake = b"\x89PNG\r\n\x1a\n";
        assert!(validate_and_hash(fake).is_err());
    }

    #[test]
    fn accepts_valid_pdf_header() {
        // Minimal syntactically valid PDF start
        let pdf = b"%PDF-1.4 fake content";
        let result = validate_and_hash(pdf);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_pdf_over_5mb_rejected() {
        let mut pdf = Vec::with_capacity(5 * 1024 * 1024 + 10);
        pdf.extend_from_slice(b"%PDF-1.4 ");
        pdf.resize(5 * 1024 * 1024 + 5, 0); // Exceeds 5MB

        let result = validate_and_hash(&pdf);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            ValidationError::FileTooLarge.to_string()
        );
    }
}
