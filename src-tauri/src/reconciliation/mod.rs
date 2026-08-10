//! Decides whether a new observation is a new transaction or an existing one.
//!
//! The same payment routinely arrives twice -- once as a bank alert, later as a
//! statement line -- so every incoming observation is matched against existing
//! canonical transactions before anything is written.
//!
//! Matching is staged for cost: a fingerprint prefilter narrows the field, exact
//! matching resolves the unambiguous cases, and only what remains is scored.
//! Where scoring is confident the records are merged; where it is not, a cluster
//! is created and the ambiguity is left for the user.
//!
//! That refusal to guess is the module's governing rule. A wrong merge destroys
//! a transaction and a wrong split inflates spending, and both are far worse
//! than asking.
pub mod alert_worker;
pub mod audit;
pub mod canonical;
pub mod cluster;
pub mod engine;
pub mod exact_match;
pub mod feedback;
pub mod post_processing;
pub mod prefilter;
pub mod scorer;

#[cfg(test)]
mod engine_tests;
