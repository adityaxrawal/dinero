//! Consent records: what the user agreed to and when.
//!
//! The app is single-user and local, so there is no login or session; the
//! session concept that used to live here guarded nothing that mattered and
//! was removed (review P5).
pub mod consent;
