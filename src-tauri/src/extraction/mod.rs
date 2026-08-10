//! Turns raw bank emails into structured transaction data.
//!
//! Extraction is layered, and `ladder` is the coordinator: cheap deterministic
//! strategies are tried first and the expensive LLM path is reached only when
//! they fall short. That ordering is the module's central design decision --
//! most bank alerts follow a stable template that a learned rule handles for
//! free, so paying for inference on every message would be both slow and
//! unnecessary.
//!
//! The supporting modules each own one concern: classifying whether a message is
//! financial at all, normalising what was extracted, resolving merchant
//! identity, detecting EMI instalments and foreign-currency fields, and
//! synthesising new rules from successful extractions so the deterministic path
//! covers more over time.
pub mod benchmark;
pub mod classifier;
pub mod currency_handler;
pub mod emi_detector;
pub mod fingerprint;
pub mod ladder;
pub mod lexicon;
#[cfg(test)]
mod lexicon_tests;
pub mod llm;
pub mod mandate_extractor;
pub mod merchant_confidence;
pub mod merchant_llm;
pub mod merchant_normalizer;
pub mod normalization;
pub mod recurring_detector;
pub mod rule_llm;
pub mod rule_synthesis;
