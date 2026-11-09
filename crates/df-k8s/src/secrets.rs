//! Kubernetes Secrets implementation of `SecretStore`.

use async_trait::async_trait;
use std::collections::BTreeMap;

use df_core::seams::{SecretError, SecretKeyRef, SecretStore};
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::Client;
use kube::api::{Api, PostParams};
use ring::rand::{SecureRandom, SystemRandom};
use secrecy::SecretString;
use tracing::instrument;

use crate::K8S_API_TIMEOUT;

/// Kubernetes-backed secret store reading Secrets from the instance namespace.
#[derive(Clone)]
pub struct K8sSecretStore {
    client: Client,
    instance_namespace: String,
}

impl K8sSecretStore {
    /// Creates a new `K8sSecretStore`.
    pub fn new(client: Client, instance_namespace: impl Into<String>) -> Self {
        Self {
            client,
            instance_namespace: instance_namespace.into(),
        }
    }

    fn api(&self) -> Api<Secret> {
        Api::namespaced(self.client.clone(), &self.instance_namespace)
    }

    /// Reads a Secret; `None` when it does not exist.
    async fn fetch(&self, r: &SecretKeyRef) -> Result<Option<Secret>, SecretError> {
        let fetched = tokio::time::timeout(K8S_API_TIMEOUT, self.api().get(&r.name))
            .await
            .map_err(|_elapsed| timeout_error("reading", r))?;
        match fetched {
            Ok(s) => Ok(Some(s)),
            Err(kube::Error::Api(err)) if err.code == 404 => Ok(None),
            Err(err) => Err(SecretError::Backend(err.to_string())),
        }
    }
}

#[async_trait]
impl SecretStore for K8sSecretStore {
    #[instrument(skip(self), fields(secret = %r.name, key = %r.key))]
    async fn get(&self, r: &SecretKeyRef) -> Result<SecretString, SecretError> {
        match self.fetch(r).await? {
            Some(secret) => extract(&secret, r),
            None => Err(SecretError::NotFound(r.clone())),
        }
    }

    #[instrument(skip(self, labels), fields(secret = %r.name, key = %r.key))]
    async fn get_or_create(
        &self,
        r: &SecretKeyRef,
        labels: &BTreeMap<String, String>,
    ) -> Result<SecretString, SecretError> {
        if let Some(secret) = self.fetch(r).await? {
            return extract(&secret, r);
        }
        let value = random_hex()?;
        let secret = Secret {
            metadata: ObjectMeta {
                name: Some(r.name.clone()),
                labels: Some(labels.clone()),
                ..ObjectMeta::default()
            },
            string_data: Some(BTreeMap::from([(r.key.clone(), value.clone())])),
            ..Secret::default()
        };
        let created = tokio::time::timeout(
            K8S_API_TIMEOUT,
            self.api().create(&PostParams::default(), &secret),
        )
        .await
        .map_err(|_elapsed| timeout_error("creating", r))?;
        match created {
            // The API returns the Secret as stored (`data`): use that, not the generated value.
            Ok(stored) => {
                tracing::info!(secret = %r.name, "created generated secret");
                extract(&stored, r)
            }
            // Another replica created it first: use its value.
            Err(kube::Error::Api(err)) if err.code == 409 => self.get(r).await,
            Err(err) => Err(SecretError::Backend(err.to_string())),
        }
    }
}

fn timeout_error(action: &str, r: &SecretKeyRef) -> SecretError {
    SecretError::Backend(format!("timeout {action} secret `{}`", r.name))
}

/// Extracts the value of `r.key` from `data` (base64 encoded `ByteString` in `k8s_openapi`).
fn extract(secret: &Secret, r: &SecretKeyRef) -> Result<SecretString, SecretError> {
    let byte_str = secret
        .data
        .as_ref()
        .and_then(|data| data.get(&r.key))
        .ok_or_else(|| SecretError::NotFound(r.clone()))?;
    let s = String::from_utf8(byte_str.0.clone())
        .map_err(|e| SecretError::Backend(format!("secret value is not UTF-8: {e}")))?;
    Ok(SecretString::from(s))
}

/// 32 random bytes, hex-encoded (64 characters).
fn random_hex() -> Result<String, SecretError> {
    let mut bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_unspecified| SecretError::Backend("random generator failed".into()))?;
    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::ByteString;
    use secrecy::ExposeSecret;
    use std::collections::BTreeMap;
    use tower_test::mock;

    #[tokio::test]
    async fn test_secret_resolution_rotated_missing_and_debug_logging() {
        let (mock_service, mut handle) = mock::pair();
        let client = Client::new(mock_service, "dark-factory-dev");
        let store = K8sSecretStore::new(client, "dark-factory-dev");

        let cred_ref = SecretKeyRef {
            name: "claude-secret".to_string(),
            key: "token".to_string(),
        };

        // 1. Initial get: returns value 1
        let get_task = {
            let store = store.clone();
            let cred_ref = cred_ref.clone();
            tokio::spawn(async move { store.get(&cred_ref).await })
        };

        let (req, send_resp) = handle.next_request().await.expect("received request 1");
        assert_eq!(
            req.uri().path(),
            "/api/v1/namespaces/dark-factory-dev/secrets/claude-secret"
        );
        let secret1 = Secret {
            data: Some(BTreeMap::from([(
                "token".to_string(),
                ByteString(b"initial-value".to_vec()),
            )])),
            ..Default::default()
        };
        send_resp.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&secret1).unwrap())
                .unwrap(),
        );

        let val1 = get_task.await.unwrap().expect("get 1 succeeded");
        assert_eq!(val1.expose_secret(), "initial-value");

        // Scenario: Debug logging
        let debug_repr = format!("{val1:?}");
        assert!(debug_repr.contains("[REDACTED]"));
        assert!(!debug_repr.contains("initial-value"));

        // 2. Scenario: Rotated Secret - value changes between runs
        let get_task2 = {
            let store = store.clone();
            let cred_ref = cred_ref.clone();
            tokio::spawn(async move { store.get(&cred_ref).await })
        };

        let (req2, send_resp2) = handle.next_request().await.expect("received request 2");
        assert_eq!(
            req2.uri().path(),
            "/api/v1/namespaces/dark-factory-dev/secrets/claude-secret"
        );
        let secret2 = Secret {
            data: Some(BTreeMap::from([(
                "token".to_string(),
                ByteString(b"rotated-value".to_vec()),
            )])),
            ..Default::default()
        };
        send_resp2.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&secret2).unwrap())
                .unwrap(),
        );

        let val2 = get_task2.await.unwrap().expect("get 2 succeeded");
        assert_eq!(val2.expose_secret(), "rotated-value");

        // 3. Scenario: Missing key
        let missing_key_ref = SecretKeyRef {
            name: "claude-secret".to_string(),
            key: "non-existent-key".to_string(),
        };

        let get_task3 = {
            let store = store.clone();
            tokio::spawn(async move { store.get(&missing_key_ref).await })
        };

        let (_req3, send_resp3) = handle.next_request().await.expect("received request 3");
        send_resp3.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&secret2).unwrap())
                .unwrap(),
        );

        let err = get_task3.await.unwrap().unwrap_err();
        assert!(matches!(
            err,
            SecretError::NotFound(ref r) if r.name == "claude-secret" && r.key == "non-existent-key"
        ));
    }

    #[tokio::test]
    async fn reading_a_secret_times_out_when_the_api_does_not_answer() {
        tokio::time::pause();
        let (mock_service, _handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let store = K8sSecretStore::new(Client::new(mock_service, "default"), "ns".to_string());
        let cred_ref = SecretKeyRef {
            name: "s".to_string(),
            key: "k".to_string(),
        };

        let err = store.get(&cred_ref).await.unwrap_err();
        assert!(
            matches!(&err, SecretError::Backend(m) if m.contains("timeout")),
            "{err}"
        );
    }

    fn json_response(status: u16, body: String) -> http::Response<String> {
        http::Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(body)
            .unwrap()
    }

    fn secret_response(key: &str, value: &str) -> http::Response<String> {
        let secret = Secret {
            data: Some(BTreeMap::from([(
                key.to_string(),
                ByteString(value.as_bytes().to_vec()),
            )])),
            ..Default::default()
        };
        json_response(200, serde_json::to_string(&secret).unwrap())
    }

    fn created_response(key: &str, value: &str) -> http::Response<String> {
        let mut response = secret_response(key, value);
        *response.status_mut() = http::StatusCode::CREATED;
        response
    }

    fn status_body(code: u16, reason: &str) -> String {
        serde_json::json!({
            "kind": "Status", "apiVersion": "v1", "status": "Failure",
            "message": reason, "reason": reason, "code": code
        })
        .to_string()
    }

    fn webhook_ref() -> SecretKeyRef {
        SecretKeyRef {
            name: "wh".to_string(),
            key: "secret".to_string(),
        }
    }

    fn labels() -> BTreeMap<String, String> {
        BTreeMap::from([(
            "app.kubernetes.io/component".into(),
            "webhook-secret".into(),
        )])
    }

    type Handle = mock::Handle<http::Request<kube::client::Body>, http::Response<String>>;

    fn spawn_get_or_create() -> (
        Handle,
        tokio::task::JoinHandle<Result<SecretString, SecretError>>,
    ) {
        let (mock_service, handle) = mock::pair();
        let store = K8sSecretStore::new(Client::new(mock_service, "ns"), "ns");
        let task =
            tokio::spawn(async move { store.get_or_create(&webhook_ref(), &labels()).await });
        (handle, task)
    }

    #[tokio::test]
    async fn get_or_create_creates_missing_secret_with_random_value_and_labels() {
        let (mut handle, task) = spawn_get_or_create();
        let (req, send) = handle.next_request().await.unwrap();
        assert_eq!(req.method(), http::Method::GET);
        send.send_response(json_response(404, status_body(404, "NotFound")));

        let (req, send) = handle.next_request().await.unwrap();
        assert_eq!(req.method(), http::Method::POST);
        assert_eq!(req.uri().path(), "/api/v1/namespaces/ns/secrets");
        let body = req.into_body().collect_bytes().await.unwrap();
        let created: Secret = serde_json::from_slice(&body).unwrap();
        assert_eq!(created.metadata.name.as_deref(), Some("wh"));
        assert_eq!(created.metadata.labels, Some(labels()));
        assert!(created.metadata.owner_references.is_none());
        let string_data = created.string_data.clone().unwrap();
        assert_eq!(string_data.len(), 1);
        let value = string_data.get("secret").unwrap();
        assert_eq!(value.len(), 64);
        assert!(value.chars().all(|c| c.is_ascii_hexdigit()));
        send.send_response(created_response("secret", value));

        let got = task.await.unwrap().unwrap();
        assert_eq!(got.expose_secret(), value);
    }

    #[tokio::test]
    async fn get_or_create_returns_the_value_the_api_stored() {
        let (mut handle, task) = spawn_get_or_create();
        let (_req, send) = handle.next_request().await.unwrap();
        send.send_response(json_response(404, status_body(404, "NotFound")));
        let (_req, send) = handle.next_request().await.unwrap();
        send.send_response(created_response("secret", "stored-by-the-api"));

        assert_eq!(
            task.await.unwrap().unwrap().expose_secret(),
            "stored-by-the-api"
        );
    }

    #[tokio::test]
    async fn get_or_create_returns_existing_secret_without_writing() {
        let (mut handle, task) = spawn_get_or_create();
        let (_req, send) = handle.next_request().await.unwrap();
        send.send_response(secret_response("secret", "mine"));

        assert_eq!(task.await.unwrap().unwrap().expose_secret(), "mine");
        assert!(handle.next_request().await.is_none(), "no further request");
    }

    #[tokio::test]
    async fn get_or_create_reads_again_when_create_conflicts() {
        let (mut handle, task) = spawn_get_or_create();
        let (_req, send) = handle.next_request().await.unwrap();
        send.send_response(json_response(404, status_body(404, "NotFound")));
        let (req, send) = handle.next_request().await.unwrap();
        assert_eq!(req.method(), http::Method::POST);
        send.send_response(json_response(409, status_body(409, "AlreadyExists")));
        let (req, send) = handle.next_request().await.unwrap();
        assert_eq!(req.method(), http::Method::GET);
        send.send_response(secret_response("secret", "winner"));

        assert_eq!(task.await.unwrap().unwrap().expose_secret(), "winner");
    }

    #[tokio::test]
    async fn get_or_create_does_not_write_when_secret_lacks_the_key() {
        let (mut handle, task) = spawn_get_or_create();
        let (_req, send) = handle.next_request().await.unwrap();
        send.send_response(secret_response("other", "x"));

        let err = task.await.unwrap().unwrap_err();
        assert!(matches!(err, SecretError::NotFound(_)), "{err}");
        assert!(handle.next_request().await.is_none(), "no write");
    }

    #[tokio::test]
    async fn get_or_create_reports_forbidden_create_as_backend_error() {
        let (mut handle, task) = spawn_get_or_create();
        let (_req, send) = handle.next_request().await.unwrap();
        send.send_response(json_response(404, status_body(404, "NotFound")));
        let (_req, send) = handle.next_request().await.unwrap();
        send.send_response(json_response(403, status_body(403, "Forbidden")));

        let err = task.await.unwrap().unwrap_err();
        assert!(
            matches!(&err, SecretError::Backend(m) if m.contains("Forbidden")),
            "{err}"
        );
    }
}
