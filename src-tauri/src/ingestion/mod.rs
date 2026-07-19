pub mod auth_results;
pub mod content_classifier;
pub mod gmail_client;
pub mod gmail_telemetry;
pub mod historical_scan;
pub mod message_processor;
pub mod mime_sanitization;
pub mod oauth;
pub mod polling;
pub mod queues;
pub mod sender_risk;
pub mod verified_senders;

#[cfg(test)]
mod polling_tests;

#[cfg(test)]
mod gmail_client_tests;

#[cfg(test)]
mod gmail_telemetry_tests;

#[cfg(test)]
mod mime_sanitization_tests;

#[cfg(test)]
mod message_processor_tests;

#[cfg(test)]
mod verified_senders_tests;

#[cfg(test)]
mod content_classifier_tests;

#[cfg(test)]
mod auth_results_tests;

#[cfg(test)]
mod sender_risk_tests;
