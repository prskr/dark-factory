//! The signing service: signs the commits of a run with its persona's key on behalf of the
//! runtime's `git-sign` shim (`specs/commit-signing/spec.md`, design D3).
//!
//! Only commit objects of the run's own persona identity are signed. Every signature and every
//! refusal is audited.

use std::sync::Arc;

use async_trait::async_trait;
use df_core::signing_key::SigningKey;
use df_core::{PersonaId, Principal, SecretStore};
use df_db::persona_keys::{KeyError, sync_signing_key};
use df_db::{DbError, NewAuditEntry, catalog, runs::get_run, write_audit_entry};
use df_orchestrator::run_context::RunContext;
use df_proto::agent::v1::{SignRequest, SignResponse, sign_response};

use crate::ChannelContext;

/// Largest payload that is signed.
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

/// A signature and the key that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// Armored SSHSIG (`-----BEGIN SSH SIGNATURE-----`), namespace `git`.
    pub armored: String,
    /// The signing key in OpenSSH format.
    pub public_key: String,
    /// `SHA256:…` of the signing key.
    pub fingerprint: String,
}

/// Why a signature could not be made.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SignError(pub String);

/// Why a request is not signed. The text is what the runtime relays to the shim and what the
/// audit entry `commit.sign_refused` records (the spec quotes some of them).
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error("the run could not be checked: {0}")]
    RunCheck(#[source] DbError),
    #[error("the run has ended")]
    RunEnded,
    #[error("the payload is larger than {MAX_PAYLOAD_BYTES} bytes")]
    PayloadTooLarge,
    #[error("the payload is not a Git commit object")]
    NotACommit,
    #[error("author does not match the persona identity")]
    AuthorMismatch,
    #[error("committer does not match the persona identity")]
    CommitterMismatch,
    #[error(transparent)]
    Signer(#[from] SignError),
    #[error("the persona could not be read: {0}")]
    PersonaRead(#[source] DbError),
    #[error("key out of sync: {0}")]
    KeyOutOfSync(#[source] KeyError),
    /// The details of the audit entry could not be serialized.
    #[error("the signature could not be audited: {0}")]
    AuditDetails(#[source] DbError),
    #[error("the signature could not be audited: {0}")]
    Audit(#[source] DbError),
}

/// Signs payloads with a persona's key. The private key never leaves the implementation.
#[async_trait]
pub trait Signer: Send + Sync {
    async fn sign(&self, persona: PersonaId, payload: &[u8]) -> Result<Signature, SignError>;
}

/// Reads the persona's key from its Kubernetes Secret on every request.
// ponytail: one Kubernetes API read per signature. Add a cache with a Secret watch only if signing
// shows up in commit times (agents commit a few times per turn).
pub struct SecretSigner {
    secrets: Arc<dyn SecretStore>,
}

impl SecretSigner {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self { secrets }
    }
}

#[async_trait]
impl Signer for SecretSigner {
    async fn sign(&self, persona: PersonaId, payload: &[u8]) -> Result<Signature, SignError> {
        let key: SigningKey = df_core::signing_key::load(self.secrets.as_ref(), persona)
            .await
            .map_err(|e| SignError(format!("the signing key is unavailable: {e}")))?;
        let armored = key.sign(payload).map_err(|e| SignError(e.to_string()))?;
        Ok(Signature {
            armored,
            public_key: key.public_key(),
            fingerprint: key.fingerprint(),
        })
    }
}

/// Answers a [`SignRequest`] of the run `rc`: the signature, or the reason nothing was signed.
pub async fn respond(ctx: &ChannelContext, rc: &RunContext, request: SignRequest) -> SignResponse {
    let signer = SecretSigner::new(Arc::clone(&ctx.secrets));
    let result = match sign_commit(ctx, rc, &signer, &request.payload).await {
        Ok(signature) => Ok(signature),
        Err(refusal) => {
            let reason = refusal.to_string();
            audit_refusal(ctx, rc, &reason).await;
            Err(reason)
        }
    };
    SignResponse {
        request_id: request.request_id,
        result: Some(match result {
            Ok(signature) => sign_response::Result::Signature(signature),
            Err(reason) => sign_response::Result::RefusedReason(reason),
        }),
    }
}

/// The checks of design D3 in order, then the signature and its audit entry.
async fn sign_commit(
    ctx: &ChannelContext,
    rc: &RunContext,
    signer: &dyn Signer,
    payload: &[u8],
) -> Result<String, Refusal> {
    let active = get_run(&ctx.pool, rc.run.id)
        .await
        .map_err(Refusal::RunCheck)?
        .is_some_and(|run| !run.state.is_terminal());
    if !active {
        return Err(Refusal::RunEnded);
    }
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Refusal::PayloadTooLarge);
    }
    let commit = parse_commit(payload)?;
    let identity = &rc.run.commit_identity;
    if !commit.author.is(&identity.name, &identity.email) {
        return Err(Refusal::AuthorMismatch);
    }
    if !commit.committer.is(&identity.name, &identity.email) {
        return Err(Refusal::CommitterMismatch);
    }
    let signature = signer.sign(rc.persona.id, payload).await?;
    reconcile_if_out_of_sync(ctx, rc, &signature).await?;
    audit_signature(ctx, rc, &signature, &commit.tree).await?;
    Ok(signature.armored)
}

/// The Secret is the source of truth: when the database holds another public key (a rotation that
/// failed after it replaced the Secret), the database takes over the Secret's key before the
/// signature is handed out.
async fn reconcile_if_out_of_sync(
    ctx: &ChannelContext,
    rc: &RunContext,
    signature: &Signature,
) -> Result<(), Refusal> {
    let stored = catalog::get_persona(&ctx.pool, rc.persona.id.as_uuid())
        .await
        .map_err(Refusal::PersonaRead)?
        .and_then(|persona| persona.signing_key)
        .map(|key| key.public_key);
    if stored.as_deref() == Some(&signature.public_key) {
        return Ok(());
    }
    sync_signing_key(&ctx.pool, ctx.secrets.as_ref(), rc.persona.id.as_uuid())
        .await
        .map(|_synced| ())
        .map_err(Refusal::KeyOutOfSync)
}

fn actor(rc: &RunContext) -> Principal {
    Principal::agent(
        rc.persona.id,
        rc.persona.display_name.clone(),
        Some(rc.run.id),
        rc.task.id,
    )
}

async fn audit_signature(
    ctx: &ChannelContext,
    rc: &RunContext,
    signature: &Signature,
    tree: &str,
) -> Result<(), Refusal> {
    let entry = NewAuditEntry::new(
        actor(rc),
        Vec::new(),
        "commit.sign",
        "run",
        rc.run.id.to_string(),
    )
    .with_details(&serde_json::json!({
        "fingerprint": signature.fingerprint,
        "tree": tree,
    }))
    .map_err(Refusal::AuditDetails)?;
    write_audit_entry(&ctx.pool, &entry)
        .await
        .map(|_row| ())
        .map_err(Refusal::Audit)
}

async fn audit_refusal(ctx: &ChannelContext, rc: &RunContext, reason: &str) {
    let entry = NewAuditEntry::new(
        actor(rc),
        Vec::new(),
        "commit.sign_refused",
        "run",
        rc.run.id.to_string(),
    )
    .with_details(&serde_json::json!({ "reason": reason }));
    let written = match entry {
        Ok(entry) => write_audit_entry(&ctx.pool, &entry)
            .await
            .map(|_row| ())
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    if let Err(error) = written {
        tracing::warn!(run_id = %rc.run.id, %error, "failed to audit a refused sign request");
    }
}

/// `Name <email>` of an author or committer line.
#[derive(Debug, PartialEq, Eq)]
struct Ident {
    name: String,
    email: String,
}

impl Ident {
    fn is(&self, name: &str, email: &str) -> bool {
        self.name == name && self.email == email
    }

    /// Parses the value of an `author` or `committer` header: `Name <email> <time> <zone>`.
    fn parse(value: &str) -> Option<Self> {
        let close = value.rfind('>')?;
        let open = value.get(..close)?.rfind('<')?;
        Some(Self {
            name: value.get(..open)?.strip_suffix(' ')?.to_string(),
            email: value.get(open + 1..close)?.to_string(),
        })
    }
}

/// Parses `value` into `slot`; `None` if it is no identity or `slot` is taken already.
fn set_once(slot: &mut Option<Ident>, value: &str) -> Option<()> {
    let ident = Ident::parse(value)?;
    slot.replace(ident).is_none().then_some(())
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedCommit {
    tree: String,
    author: Ident,
    committer: Ident,
}

/// Parses the headers of a Git commit object. A tag, a tree, a blob or arbitrary bytes are not
/// commits; a commit with a second `author` or `committer` header is refused, so no header can
/// hide behind the one that was checked.
fn parse_commit(payload: &[u8]) -> Result<ParsedCommit, Refusal> {
    let headers = payload
        .split(|byte| *byte == b'\n')
        .take_while(|line| !line.is_empty());
    let (mut tree, mut author, mut committer) = (None, None, None);
    for (index, line) in headers.enumerate() {
        let line = std::str::from_utf8(line).map_err(|_not_utf8| Refusal::NotACommit)?;
        if index == 0 {
            let id = line.strip_prefix("tree ").ok_or(Refusal::NotACommit)?;
            if !(matches!(id.len(), 40 | 64) && id.bytes().all(|b| b.is_ascii_hexdigit())) {
                return Err(Refusal::NotACommit);
            }
            tree = Some(id.to_string());
        } else if let Some(value) = line.strip_prefix("author ") {
            set_once(&mut author, value).ok_or(Refusal::NotACommit)?;
        } else if let Some(value) = line.strip_prefix("committer ") {
            set_once(&mut committer, value).ok_or(Refusal::NotACommit)?;
        }
    }
    match (tree, author, committer) {
        (Some(tree), Some(author), Some(committer)) => Ok(ParsedCommit {
            tree,
            author,
            committer,
        }),
        _ => Err(Refusal::NotACommit),
    }
}

#[cfg(test)]
mod tests;
