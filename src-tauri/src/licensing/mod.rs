//! Subscription entitlement: activation, validation and enforcement.
//!
//! Entitlement is proved by an RS256-signed token from the licensing service.
//! The app holds only the public key, so it can verify a licence offline but
//! cannot mint one -- which is what allows the product to keep working without a
//! network connection while remaining tamper-evident.
//!
//! The state machine handles the transitions that matter in practice: a trial
//! expiring, a payment failing into a grace period, and grace elapsing into a
//! lock. Grace exists because a failed card should not instantly destroy access
//! for a paying customer.
pub mod client;
pub mod commands;
pub mod device;
pub mod gate;
pub mod jwt;
pub mod state;
pub mod state_machine;
pub mod worker;
