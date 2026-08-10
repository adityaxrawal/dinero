//! Validates an uploaded file before any parsing is attempted.
//!
//! The trust boundary for a user-supplied document: type and size are checked
//! before the bytes reach a parser, and the content hash computed here is what
//! duplicate detection is keyed on.
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("invalid_file_type: file must be application/pdf")]
    InvalidFileType,
    #[error("file_too_large: file must not exceed 5 MB")]
    FileTooLarge,
    #[error("empty_file: file must not be zero bytes")]
    EmptyFile,
}

/// Validates an uploaded file and returns its content hash.
///
/// The trust boundary for user-supplied documents. Type is established from the
/// `%PDF` magic bytes rather than the filename or a client-supplied MIME string,
/// neither of which an attacker would find hard to set. The size cap bounds what
/// a malicious file can make the parser allocate.
///
/// The returned hash is what duplicate detection is keyed on.
pub fn validate_and_hash(bytes: &[u8]) -> Result<String> {
    if bytes.is_empty() {
        return Err(anyhow!(ValidationError::EmptyFile));
    }

    if bytes.len() < 4 || &bytes[..4] != b"%PDF" {
        return Err(anyhow!(ValidationError::InvalidFileType));
    }

    const MAX_BYTES: usize = 5 * 1024 * 1024;
    if bytes.len() > MAX_BYTES {
        return Err(anyhow!(ValidationError::FileTooLarge));
    }

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
        let fake = b"\x89PNG\r\n\x1a\n";
        assert!(validate_and_hash(fake).is_err());
    }

    #[test]
    fn accepts_valid_pdf_header() {
        let pdf = b"%PDF-1.4 fake content";
        let result = validate_and_hash(pdf);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 64);
    }

    #[test]
    fn test_pdf_over_5mb_rejected() {
        let mut pdf = Vec::with_capacity(5 * 1024 * 1024 + 10);
        pdf.extend_from_slice(b"%PDF-1.4 ");
        pdf.resize(5 * 1024 * 1024 + 5, 0);

        let result = validate_and_hash(&pdf);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            ValidationError::FileTooLarge.to_string()
        );
    }
}
