//! Local session handling and consent records.
//!
//! Not authentication against a server -- the app is single-user and local.
//! Sessions scope activity for auditing and give incident response something to
//! revoke; consent records capture what the user agreed to and when.
pub mod consent;
pub mod session;
