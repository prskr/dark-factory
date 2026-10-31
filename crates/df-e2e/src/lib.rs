//! End-to-end suite for the iteration 1 demo (`docs/demo.md`) and the seed program with its
//! well-known data.
//!
//! The tests (`tests/`) act as a board member through the web UI and read everything the UI does
//! not show from Forgejo and the database. All targets come from the environment ([`E2eEnv`]),
//! nothing here is specific to the kind cluster of `just e2e-local`.

pub mod browser;
pub mod data;
pub mod db;
pub mod env;
pub mod forgejo;
pub mod seed;
pub mod wait;

pub use env::E2eEnv;
