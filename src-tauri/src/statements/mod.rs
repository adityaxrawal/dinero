//! PDF statement ingestion, from an uploaded file to reviewable rows.
//!
//! The pipeline runs: validate and hash the file, check it is not a duplicate,
//! unlock it if encrypted, extract text, reconstruct the page layout, pull
//! transaction rows out of that layout, and stage them as a draft for review.
//!
//! Two constraints shape the whole module. Statement PDFs are the most sensitive
//! documents this app handles, so parsing happens in memory and stored copies
//! are short-lived and purged. And parsing runs in a sandboxed sidecar process
//! rather than in-process, because PDF parsers are a well-known source of
//! memory-safety bugs and the input here is an untrusted file.
pub mod bill_classifier;
pub mod display_name;
pub mod duplicate_check;
pub mod events;
pub mod layout;
pub mod learned_rows;
pub mod metadata_extractor;
pub mod observation_builder;
pub mod parser;
pub mod password;
pub mod pdf_storage;
pub mod row_extractor;
pub mod row_llm;
pub mod sidecar;
pub mod validator;
