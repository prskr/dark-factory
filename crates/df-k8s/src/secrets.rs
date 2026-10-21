//! Kubernetes Secrets implementation of `SecretStore`.

use async_trait::async_trait;
use df_core::seams::{CredentialRef, SecretError, SecretStore};
use k8s_openapi::api::core::v1::Secret;
use kube::Api;
use kube::Client;
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
}

#[async_trait]
impl SecretStore for K8sSecretStore {
    #[instrument(skip(self), fields(secret = %r.secret_name, key = %r.secret_key))]
    async fn get(&self, r: &CredentialRef) -> Result<SecretString, SecretError> {
        let secrets: Api<Secret> = Api::namespaced(self.client.clone(), &self.instance_namespace);
        let fetched = tokio::time::timeout(K8S_API_TIMEOUT, secrets.get(&r.secret_name))
            .await
            .map_err(|_| {
                SecretError::Backend(format!("timeout reading secret `{}`", r.secret_name))
            })?;
        let secret = match fetched {
            Ok(s) => s,
            Err(kube::Error::Api(err)) if err.code == 404 => {
                return Err(SecretError::NotFound {
                    secret_name: r.secret_name.clone(),
                    secret_key: r.secret_key.clone(),
                });
            }
            Err(err) => return Err(SecretError::Backend(err.to_string())),
        };

        // Extract from `data` (base64 encoded ByteString in k8s_openapi)
        if let Some(ref data) = secret.data
            && let Some(byte_str) = data.get(&r.secret_key)
        {
            let s = String::from_utf8(byte_str.0.clone())
                .map_err(|e| SecretError::Backend(format!("secret value is not UTF-8: {e}")))?;
            return Ok(SecretString::from(s));
        }

        Err(SecretError::NotFound {
            secret_name: r.secret_name.clone(),
            secret_key: r.secret_key.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_core::ids::CredentialRefId;
    use k8s_openapi::ByteString;
    use secrecy::ExposeSecret;
    use std::collections::BTreeMap;
    use tower_test::mock;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_secret_resolution_rotated_missing_and_debug_logging() {
        let (mock_service, mut handle) = mock::pair();
        let client = Client::new(mock_service, "dark-factory-dev");
        let store = K8sSecretStore::new(client, "dark-factory-dev");

        let cred_ref = CredentialRef {
            id: CredentialRefId::from_uuid(Uuid::from_u128(1)),
            name: "claude-token".to_string(),
            secret_name: "claude-secret".to_string(),
            secret_key: "token".to_string(),
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
        let missing_key_ref = CredentialRef {
            id: CredentialRefId::from_uuid(Uuid::from_u128(2)),
            name: "bad-ref".to_string(),
            secret_name: "claude-secret".to_string(),
            secret_key: "non-existent-key".to_string(),
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
            SecretError::NotFound {
                ref secret_name,
                ref secret_key,
            } if secret_name == "claude-secret" && secret_key == "non-existent-key"
        ));
    }

    #[tokio::test]
    async fn reading_a_secret_times_out_when_the_api_does_not_answer() {
        tokio::time::pause();
        let (mock_service, _handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let store = K8sSecretStore::new(Client::new(mock_service, "default"), "ns".to_string());
        let cred_ref = CredentialRef {
            id: CredentialRefId::from_uuid(Uuid::from_u128(3)),
            name: "n".to_string(),
            secret_name: "s".to_string(),
            secret_key: "k".to_string(),
        };

        let err = store.get(&cred_ref).await.unwrap_err();
        assert!(
            matches!(&err, SecretError::Backend(m) if m.contains("timeout")),
            "{err}"
        );
    }
}
