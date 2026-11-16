//! What `just e2e-local` prepares before the tests run, as code under `cargo test`: the Forgejo
//! instance ([`forgejo`]) and the check that the Playwright versions agree ([`versions`]). The
//! `setup` binary (`src/bin/setup.rs`) is the command line around it.

pub mod forgejo;
pub mod versions;
