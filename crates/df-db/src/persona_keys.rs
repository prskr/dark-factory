//! The signing key of a persona: the private key in a Kubernetes Secret, its public half in
//! `personas.signing_public_key` and `signing_fingerprint` (`specs/commit-signing/spec.md`, design
//! D4).
//!
//! The Secret is the source of truth. Both changes (Secret and columns) run under a row lock of the
//! persona, so concurrent rotations and reconciles of one persona end on the same key.

use df_core::signing_key::{SigningKey, SigningKeyError, secret_labels, secret_ref};
use df_core::{PersonaId, Principal, RunState, SecretError, SecretStore};
use secrecy::SecretString;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::catalog::Actor;
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::runs::state_list;

/// The public half of a persona's signing key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PublicSigningKey {
    /// OpenSSH format, `ssh-ed25519 AAAA…`.
    pub public_key: String,
    /// `SHA256:…`.
    pub fingerprint: String,
}

impl From<&SigningKey> for PublicSigningKey {
    fn from(key: &SigningKey) -> Self {
        Self {
            public_key: key.public_key(),
            fingerprint: key.fingerprint(),
        }
    }
}

/// Why a key could not be created, reconciled or rotated.
#[derive(Debug, Error)]
pub enum KeyError {
    #[error("{0}")]
    Db(#[from] DbError),
    #[error("{0}")]
    Key(#[from] SigningKeyError),
    #[error("{0}")]
    Secret(#[from] SecretError),
}

impl From<sqlx::Error> for KeyError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e.into())
    }
}

/// What the `personas` row holds of the key.
struct StoredKey {
    handle: String,
    fingerprint: Option<String>,
    public_key: Option<String>,
}

/// Makes the database agree with the Secret of persona `id`: creates the key when there is none,
/// and takes over the Secret's key when the database holds another one (a rotation that failed
/// after it replaced the Secret). Replacing a key writes the audit entry
/// `persona.signing_key.reconcile` and publishes `egress_policy`.
pub async fn sync_signing_key(
    pool: &PgPool,
    secrets: &dyn SecretStore,
    id: Uuid,
) -> Result<PublicSigningKey, KeyError> {
    let mut tx = pool.begin().await?;
    let stored = lock_persona(&mut tx, id).await?;
    let key = secret_key(secrets, id).await?;
    let current = PublicSigningKey::from(&key);
    if stored.public_key.as_deref() == Some(&current.public_key) {
        return Ok(current);
    }
    store_public_key(&mut tx, id, &current).await?;
    if let Some(old) = &stored.fingerprint {
        let entry = audit_entry(
            Principal::system("persona.signing_key.reconcile"),
            Vec::new(),
            "persona.signing_key.reconcile",
            &stored.handle,
            id,
            old,
            &current.fingerprint,
        )?;
        write_audit_entry(&mut *tx, &entry).await?;
        publish(&mut *tx, &Notification::EgressPolicy).await?;
    }
    tx.commit().await?;
    Ok(current)
}

/// Replaces the signing key of persona `id` with a new one: Secret and columns, the audit entry
/// `persona.signing_key.rotate` with both fingerprints, and `egress_policy`, in one transaction.
/// A failed Secret write rolls everything back.
pub async fn rotate_signing_key(
    pool: &PgPool,
    secrets: &dyn SecretStore,
    actor: Actor<'_>,
    id: Uuid,
) -> Result<PublicSigningKey, KeyError> {
    let mut tx = pool.begin().await?;
    let stored = lock_persona(&mut tx, id).await?;
    let new = SigningKey::generate()?;
    let r = secret_ref(PersonaId::from_uuid(id));
    match secrets.replace(&r, new.private_openssh()).await {
        Err(SecretError::NotFound(_)) => {
            // A persona without a key: rotating creates it.
            secrets
                .get_or_create_with(&r, &secret_labels(), new.private_openssh())
                .await?;
        }
        other => other?,
    }
    let public = PublicSigningKey::from(&new);
    store_public_key(&mut tx, id, &public).await?;
    let entry = audit_entry(
        actor.principal.clone(),
        actor.roles.to_vec(),
        "persona.signing_key.rotate",
        &stored.handle,
        id,
        stored.fingerprint.as_deref().unwrap_or_default(),
        &public.fingerprint,
    )?;
    write_audit_entry(&mut *tx, &entry).await?;
    publish(&mut *tx, &Notification::EgressPolicy).await?;
    tx.commit().await?;
    Ok(public)
}

/// The number of runs of persona `id` that have not ended: a rotation affects their unpushed
/// commits.
pub async fn active_run_count(pool: &PgPool, id: Uuid) -> Result<u64, DbError> {
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM runs r JOIN tasks t ON t.id = r.task_id
           WHERE t.persona_id = $1 AND r.state <> ALL($2)"#,
        id,
        &state_list(RunState::TERMINAL)
    )
    .fetch_one(pool)
    .await?;
    Ok(u64::try_from(count).unwrap_or_default())
}

/// The persona's key from its Secret, generated first when the Secret does not exist.
async fn secret_key(secrets: &dyn SecretStore, id: Uuid) -> Result<SigningKey, KeyError> {
    let r = secret_ref(PersonaId::from_uuid(id));
    let value: SecretString = match secrets.get(&r).await {
        Ok(value) => value,
        Err(SecretError::NotFound(_)) => {
            let generated = SigningKey::generate()?;
            secrets
                .get_or_create_with(&r, &secret_labels(), generated.private_openssh())
                .await?
        }
        Err(e) => return Err(e.into()),
    };
    Ok(SigningKey::from_openssh(&value)?)
}

async fn lock_persona(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<StoredKey, KeyError> {
    sqlx::query!(
        "SELECT handle, signing_public_key, signing_fingerprint
         FROM personas WHERE id = $1 FOR UPDATE",
        id
    )
    .fetch_optional(&mut **tx)
    .await?
    .map(|row| StoredKey {
        handle: row.handle,
        fingerprint: row.signing_fingerprint,
        public_key: row.signing_public_key,
    })
    .ok_or_else(|| KeyError::Db(DbError::NotFound(format!("persona `{id}`"))))
}

async fn store_public_key(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    key: &PublicSigningKey,
) -> Result<(), KeyError> {
    sqlx::query!(
        "UPDATE personas SET signing_public_key = $2, signing_fingerprint = $3 WHERE id = $1",
        id,
        key.public_key,
        key.fingerprint
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn audit_entry(
    actor: Principal,
    roles: Vec<String>,
    action: &str,
    handle: &str,
    id: Uuid,
    old_fingerprint: &str,
    new_fingerprint: &str,
) -> Result<NewAuditEntry, DbError> {
    NewAuditEntry::new(actor, roles, action, "persona", handle).with_details(&serde_json::json!({
        "id": id,
        "old_fingerprint": old_fingerprint,
        "new_fingerprint": new_fingerprint,
    }))
}

#[cfg(test)]
mod tests;
