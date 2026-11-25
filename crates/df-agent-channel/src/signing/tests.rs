//! Tests of the signing service: the guards of design D3, the audit entries and the reconcile of a
//! key that is out of sync.

use df_core::signing_key::{SigningKey, secret_ref, verify};
use df_db::Notification;
use df_db::persona_keys::sync_signing_key;
use df_orchestrator::run_context::load_run_context;
use df_proto::agent::v1::sign_response::Result as Outcome;

use super::*;
use crate::testing::Env;

const TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

fn commit(author: &str, committer: &str) -> Vec<u8> {
    format!(
        "tree {TREE}\nauthor {author} 1700000000 +0000\ncommitter {committer} 1700000000 +0000\n\nFix login\n"
    )
    .into_bytes()
}

fn linus() -> Vec<u8> {
    commit("Linus <linus@example.org>", "Linus <linus@example.org>")
}

/// A running run of persona Linus (identity `Linus <linus@example.org>`) with its signing key.
async fn running() -> (Env, RunContext, df_db::persona_keys::PublicSigningKey) {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let key = sync_signing_key(&pool, env.ctx.secrets.as_ref(), env.persona_id.as_uuid())
        .await
        .unwrap();
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    let run = get_run(&pool, run_id).await.unwrap().unwrap();
    let rc = load_run_context(&pool, run).await.unwrap();
    (env, rc, key)
}

async fn ask(env: &Env, rc: &RunContext, payload: Vec<u8>) -> Outcome {
    let request = SignRequest {
        request_id: "r-1".into(),
        payload: payload.into(),
    };
    let response = respond(&env.ctx, rc, request).await;
    assert_eq!(response.request_id, "r-1");
    response.result.unwrap()
}

async fn audit(env: &Env, action: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT details FROM audit_log WHERE action = $1 ORDER BY id")
        .bind(action)
        .fetch_all(&env.ctx.pool)
        .await
        .unwrap()
}

fn refusal(outcome: Outcome) -> String {
    match outcome {
        Outcome::RefusedReason(reason) => reason,
        Outcome::Signature(_) => panic!("signed"),
    }
}

#[tokio::test]
async fn a_valid_commit_is_signed_and_the_signature_verifies_and_is_audited() {
    let (env, rc, key) = running().await;

    let Outcome::Signature(armored) = ask(&env, &rc, linus()).await else {
        panic!("refused");
    };

    verify(&key.public_key, &linus(), armored.as_bytes()).unwrap();
    let signed = audit(&env, "commit.sign").await;
    assert_eq!(signed.len(), 1);
    assert_eq!(signed[0]["fingerprint"], key.fingerprint);
    assert_eq!(signed[0]["tree"], TREE);
    let (actor, resource): (serde_json::Value, String) =
        sqlx::query_as("SELECT actor, resource_id FROM audit_log WHERE action = 'commit.sign'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert_eq!(actor["kind"], "agent");
    assert_eq!(actor["run_id"], rc.run.id.to_string());
    assert_eq!(actor["task_id"], rc.task.id.to_string());
    assert_eq!(resource, rc.run.id.to_string());
    assert!(audit(&env, "commit.sign_refused").await.is_empty());
}

#[tokio::test]
async fn a_commit_of_another_author_or_committer_is_refused_and_audited() {
    let (env, rc, _) = running().await;

    let author = refusal(
        ask(
            &env,
            &rc,
            commit("Ada <ada@example.org>", "Linus <linus@example.org>"),
        )
        .await,
    );
    let committer = refusal(
        ask(
            &env,
            &rc,
            commit("Linus <linus@example.org>", "Linus <other@example.org>"),
        )
        .await,
    );

    assert_eq!(author, "author does not match the persona identity");
    assert_eq!(committer, "committer does not match the persona identity");
    assert!(audit(&env, "commit.sign").await.is_empty());
    let refused = audit(&env, "commit.sign_refused").await;
    assert_eq!(refused.len(), 2);
    assert_eq!(refused[0]["reason"], author);
}

#[tokio::test]
async fn a_tag_and_random_bytes_are_refused() {
    let (env, rc, _) = running().await;
    let tag = format!(
        "object {TREE}\ntype commit\ntag v1\ntagger Linus <linus@example.org> 1 +0000\n\nrelease\n"
    );

    for payload in [tag.into_bytes(), vec![0, 159, 146, 150], b"hello".to_vec()] {
        let reason = refusal(ask(&env, &rc, payload).await);
        assert_eq!(reason, "the payload is not a Git commit object");
    }
    assert!(audit(&env, "commit.sign").await.is_empty());
}

#[tokio::test]
async fn a_commit_with_a_repeated_author_header_is_refused() {
    let (env, rc, _) = running().await;
    let payload = format!(
        "tree {TREE}\nauthor Linus <linus@example.org> 1 +0000\nauthor Ada <ada@example.org> 1 +0000\ncommitter Linus <linus@example.org> 1 +0000\n\nm\n"
    );

    let reason = refusal(ask(&env, &rc, payload.into_bytes()).await);

    assert_eq!(reason, "the payload is not a Git commit object");
}

#[tokio::test]
async fn a_payload_over_one_mebibyte_is_refused() {
    let (env, rc, _) = running().await;
    let mut payload = linus();
    payload.extend(std::iter::repeat_n(b'x', MAX_PAYLOAD_BYTES));

    let reason = refusal(ask(&env, &rc, payload).await);

    assert!(reason.contains("larger than"), "{reason}");
}

#[tokio::test]
async fn a_request_of_an_ended_run_is_refused() {
    let (env, rc, _) = running().await;
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(rc.run.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    let reason = refusal(ask(&env, &rc, linus()).await);

    assert_eq!(reason, "the run has ended");
    assert!(audit(&env, "commit.sign").await.is_empty());
}

#[tokio::test]
async fn a_key_out_of_sync_is_reconciled_from_the_secret_before_signing() {
    let (env, rc, old) = running().await;
    // A rotation replaced the Secret and then failed to store the public key.
    let replaced = SigningKey::generate().unwrap();
    env.secrets
        .replace(&secret_ref(env.persona_id), replaced.private_openssh())
        .await
        .unwrap();
    let mut notes = env.ctx.notifier.subscribe();

    let Outcome::Signature(armored) = ask(&env, &rc, linus()).await else {
        panic!("refused");
    };

    verify(&replaced.public_key(), &linus(), armored.as_bytes()).unwrap();
    let stored: Option<String> =
        sqlx::query_scalar("SELECT signing_public_key FROM personas WHERE id = $1")
            .bind(env.persona_id.as_uuid())
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert_eq!(stored, Some(replaced.public_key()));
    let reconciled = audit(&env, "persona.signing_key.reconcile").await;
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0]["old_fingerprint"], old.fingerprint);
    assert_eq!(
        audit(&env, "commit.sign").await[0]["fingerprint"],
        replaced.fingerprint()
    );
    let published = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if matches!(notes.recv().await, Ok(Notification::EgressPolicy)) {
                return;
            }
        }
    })
    .await;
    assert!(published.is_ok(), "egress_policy was published");
}

/// Signs with a key that is not the one in the Secret.
struct OtherKeySigner(SigningKey);

#[async_trait]
impl Signer for OtherKeySigner {
    async fn sign(&self, _persona: PersonaId, payload: &[u8]) -> Result<Signature, SignError> {
        Ok(Signature {
            armored: self.0.sign(payload).unwrap(),
            public_key: self.0.public_key(),
            fingerprint: self.0.fingerprint(),
        })
    }
}

#[tokio::test]
async fn a_signature_is_refused_when_the_reconcile_fails() {
    let (env, rc, _) = running().await;
    let secret = secret_ref(env.persona_id);
    env.secrets.insert_error(
        secret.name,
        secret.key,
        df_core::SecretError::Backend("unreachable".into()),
    );
    let signer = OtherKeySigner(SigningKey::generate().unwrap());

    let reason = sign_commit(&env.ctx, &rc, &signer, &linus())
        .await
        .unwrap_err()
        .to_string();

    assert!(reason.starts_with("key out of sync"), "{reason}");
    assert!(audit(&env, "commit.sign").await.is_empty());
}

#[test]
fn identities_are_parsed_from_author_lines() {
    let ident = Ident::parse("Linus T <linus@example.org> 1700000000 +0100").unwrap();
    assert!(ident.is("Linus T", "linus@example.org"));
    assert!(Ident::parse("no email 1 +0000").is_none());
}
