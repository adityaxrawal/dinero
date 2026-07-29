//! The closed feedback loop: a user correction in, a validated extraction rule
//! out (design 2026-07-29).
//!
//! Split from `ingestion::queues` on purpose. Those queues carry the ingest
//! critical path, where a stall is a visible scan stall; this one carries
//! best-effort learning, where dropping a job costs nothing worse than not
//! learning from one correction. Mixing the two would make the ingest queues'
//! backpressure semantics answer for work that must never apply backpressure.

pub mod worker;

pub use worker::{enqueue, spawn_learning_worker, FeedbackJob, LearningHandle};
