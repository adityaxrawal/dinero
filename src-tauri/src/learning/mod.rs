//! Turns user corrections into automatic future behaviour.
//!
//! Every manual fix is evidence about this user's actual banks and merchants.
//! Learning runs on a background worker rather than inline, so correcting a
//! transaction stays instant while rule synthesis happens afterwards.
pub mod worker;

pub use worker::{enqueue, spawn_learning_worker, FeedbackJob, LearningHandle};
