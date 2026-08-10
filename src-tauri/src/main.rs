//! Desktop binary entry point.
//!
//! Deliberately empty of logic: everything lives in the library crate so that
//! the same code is reachable from integration tests and other binaries, which
//! cannot link against a `main.rs`.

// Suppresses the console window that Windows would otherwise attach to a
// release build. Left enabled in debug builds, where stdout is wanted.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dinero_app_lib::run()
}
