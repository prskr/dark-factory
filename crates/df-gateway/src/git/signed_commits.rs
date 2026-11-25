//! Test fixture shared by the push check tests: commit objects signed by a persona key.

use std::sync::LazyLock;

use df_core::signing_key::{SigningKey, SplitCommit};

/// The signing key of the run's persona.
pub(crate) static PERSONA_KEY: LazyLock<SigningKey> =
    LazyLock::new(|| SigningKey::generate().unwrap());

/// A commit object `n` signed by `key`.
pub(crate) fn signed_by(key: &SigningKey, n: u8) -> SplitCommit {
    let payload = format!("commit {n}").into_bytes();
    let signature = Some(key.sign(&payload).unwrap().into_bytes());
    SplitCommit { payload, signature }
}
