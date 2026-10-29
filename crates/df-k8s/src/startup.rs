//! Startup verification of the sandbox namespace and control plane RBAC permissions.

use df_core::config::ClusterTargetsConfig;
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use k8s_openapi::api::core::v1::Namespace;
use kube::{Api, Client};
use thiserror::Error;
use tracing::info;

use crate::K8S_API_TIMEOUT;

/// Namespace shown in errors for cluster-wide permission checks.
pub const ALL_NAMESPACES: &str = "*";

/// Required permission attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequiredPermission {
    pub group: &'static str,
    pub resource: &'static str,
    pub verb: &'static str,
}

/// Required permissions checked in the sandbox namespace at startup.
pub const SANDBOX_PERMISSIONS: &[RequiredPermission] = &[
    RequiredPermission {
        group: "agents.x-k8s.io",
        resource: "sandboxes",
        verb: "create",
    },
    RequiredPermission {
        group: "agents.x-k8s.io",
        resource: "sandboxes",
        verb: "get",
    },
    RequiredPermission {
        group: "agents.x-k8s.io",
        resource: "sandboxes",
        verb: "list",
    },
    RequiredPermission {
        group: "agents.x-k8s.io",
        resource: "sandboxes",
        verb: "watch",
    },
    RequiredPermission {
        group: "agents.x-k8s.io",
        resource: "sandboxes",
        verb: "patch",
    },
    RequiredPermission {
        group: "agents.x-k8s.io",
        resource: "sandboxes",
        verb: "delete",
    },
    RequiredPermission {
        group: "",
        resource: "persistentvolumeclaims",
        verb: "create",
    },
    RequiredPermission {
        group: "",
        resource: "persistentvolumeclaims",
        verb: "get",
    },
    RequiredPermission {
        group: "",
        resource: "persistentvolumeclaims",
        verb: "list",
    },
    RequiredPermission {
        group: "",
        resource: "persistentvolumeclaims",
        verb: "watch",
    },
    RequiredPermission {
        group: "",
        resource: "persistentvolumeclaims",
        verb: "patch",
    },
    RequiredPermission {
        group: "",
        resource: "persistentvolumeclaims",
        verb: "delete",
    },
    RequiredPermission {
        group: "networking.k8s.io",
        resource: "networkpolicies",
        verb: "create",
    },
    RequiredPermission {
        group: "networking.k8s.io",
        resource: "networkpolicies",
        verb: "get",
    },
    RequiredPermission {
        group: "networking.k8s.io",
        resource: "networkpolicies",
        verb: "list",
    },
    RequiredPermission {
        group: "networking.k8s.io",
        resource: "networkpolicies",
        verb: "watch",
    },
    RequiredPermission {
        group: "networking.k8s.io",
        resource: "networkpolicies",
        verb: "patch",
    },
    RequiredPermission {
        group: "networking.k8s.io",
        resource: "networkpolicies",
        verb: "delete",
    },
    RequiredPermission {
        group: "",
        resource: "pods",
        verb: "get",
    },
    RequiredPermission {
        group: "",
        resource: "pods",
        verb: "list",
    },
    RequiredPermission {
        group: "",
        resource: "pods",
        verb: "watch",
    },
];

/// Required permissions checked in the instance namespace at startup (e.g. Lease coordination, Services).
pub const INSTANCE_PERMISSIONS: &[RequiredPermission] = &[
    RequiredPermission {
        group: "coordination.k8s.io",
        resource: "leases",
        verb: "create",
    },
    RequiredPermission {
        group: "coordination.k8s.io",
        resource: "leases",
        verb: "get",
    },
    RequiredPermission {
        group: "coordination.k8s.io",
        resource: "leases",
        verb: "update",
    },
    RequiredPermission {
        group: "",
        resource: "services",
        verb: "get",
    },
    RequiredPermission {
        group: "",
        resource: "secrets",
        verb: "get",
    },
];

/// Required permissions checked in every cluster-target namespace (or cluster-wide with
/// `clusterTargets.anyNamespace`): resolving Services and watching Services and `EndpointSlices`.
pub const TARGET_PERMISSIONS: &[RequiredPermission] = &[
    RequiredPermission {
        group: "",
        resource: "services",
        verb: "get",
    },
    RequiredPermission {
        group: "",
        resource: "services",
        verb: "list",
    },
    RequiredPermission {
        group: "",
        resource: "services",
        verb: "watch",
    },
    RequiredPermission {
        group: "discovery.k8s.io",
        resource: "endpointslices",
        verb: "list",
    },
    RequiredPermission {
        group: "discovery.k8s.io",
        resource: "endpointslices",
        verb: "watch",
    },
];

/// Errors returned by the startup verification check.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StartupCheckError {
    #[error("configured sandbox namespace `{0}` does not exist")]
    NamespaceNotFound(String),
    /// Every missing permission of every checked namespace, each as
    /// `<verb> <resource>[.<group>] in namespace <ns>`.
    #[error("missing required permissions: {}", .missing.join("; "))]
    MissingPermissions { missing: Vec<String> },
    #[error("kubernetes API error during startup check: {0}")]
    Api(String),
}

/// Verifies that `sandbox_namespace` exists and that the client holds all required permissions
/// in the sandbox namespace, the instance namespace and every cluster-target namespace.
pub async fn verify_startup(
    client: &Client,
    sandbox_namespace: &str,
    instance_namespace: &str,
    cluster_targets: &ClusterTargetsConfig,
) -> Result<(), StartupCheckError> {
    // 1. Verify sandbox namespace exists
    let namespaces: Api<Namespace> = Api::all(client.clone());
    let get_fut = tokio::time::timeout(K8S_API_TIMEOUT, namespaces.get(sandbox_namespace));
    match get_fut.await {
        Ok(Ok(_)) => {}
        Ok(Err(kube::Error::Api(err))) if err.code == 404 => {
            return Err(StartupCheckError::NamespaceNotFound(
                sandbox_namespace.to_string(),
            ));
        }
        // The chart grants `get` on exactly this namespace, so 403 means the grant is missing.
        Ok(Err(kube::Error::Api(err))) if err.code == 403 => {
            return Err(StartupCheckError::MissingPermissions {
                missing: vec![format!("get namespaces/{sandbox_namespace} (cluster-wide)")],
            });
        }
        Ok(Err(err)) => return Err(StartupCheckError::Api(err.to_string())),
        Err(_) => {
            return Err(StartupCheckError::Api(format!(
                "timeout checking existence of sandbox namespace `{sandbox_namespace}`"
            )));
        }
    }

    // 2. SelfSubjectAccessReview per namespace and permission set
    let ssar_api: Api<SelfSubjectAccessReview> = Api::all(client.clone());
    let mut checks = vec![
        (Some(sandbox_namespace), SANDBOX_PERMISSIONS),
        (Some(instance_namespace), INSTANCE_PERMISSIONS),
    ];
    if cluster_targets.any_namespace {
        checks.push((None, TARGET_PERMISSIONS));
    } else {
        checks.extend(
            cluster_targets
                .namespaces
                .iter()
                .map(|ns| (Some(ns.as_str()), TARGET_PERMISSIONS)),
        );
    }
    // Check everything first so the error names every gap, not only the first namespace's.
    let mut missing = Vec::new();
    for (namespace, permissions) in checks {
        let ns = namespace.unwrap_or(ALL_NAMESPACES);
        missing.extend(
            check_permissions(&ssar_api, namespace, permissions)
                .await?
                .into_iter()
                .map(|perm| format!("{perm} in namespace `{ns}`")),
        );
    }
    if !missing.is_empty() {
        return Err(StartupCheckError::MissingPermissions { missing });
    }

    info!(
        sandbox_namespace,
        instance_namespace,
        target_namespaces = cluster_targets.namespaces.len(),
        any_target_namespace = cluster_targets.any_namespace,
        "sandbox, instance and cluster-target RBAC permissions verified"
    );
    Ok(())
}

async fn check_permissions(
    ssar_api: &Api<SelfSubjectAccessReview>,
    namespace: Option<&str>,
    permissions: &[RequiredPermission],
) -> Result<Vec<String>, StartupCheckError> {
    let mut missing = Vec::new();
    for perm in permissions {
        let req = SelfSubjectAccessReview {
            spec: SelfSubjectAccessReviewSpec {
                resource_attributes: Some(ResourceAttributes {
                    namespace: namespace.map(str::to_string),
                    group: if perm.group.is_empty() {
                        None
                    } else {
                        Some(perm.group.to_string())
                    },
                    resource: Some(perm.resource.to_string()),
                    verb: Some(perm.verb.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let pp = kube::api::PostParams::default();
        let create_fut = tokio::time::timeout(K8S_API_TIMEOUT, ssar_api.create(&pp, &req));
        match create_fut.await {
            Ok(Ok(review)) => {
                let allowed = review.status.is_some_and(|s| s.allowed);
                if !allowed {
                    let desc = if perm.group.is_empty() {
                        format!("{} {}", perm.verb, perm.resource)
                    } else {
                        format!("{} {}.{}", perm.verb, perm.resource, perm.group)
                    };
                    missing.push(desc);
                }
            }
            Ok(Err(err)) => return Err(StartupCheckError::Api(err.to_string())),
            Err(_) => {
                return Err(StartupCheckError::Api(format!(
                    "timeout checking permission {} {} in {}",
                    perm.verb,
                    perm.resource,
                    namespace.unwrap_or(ALL_NAMESPACES)
                )));
            }
        }
    }
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::authorization::v1::SubjectAccessReviewStatus;
    use tower_test::mock;

    #[tokio::test]
    async fn test_verify_startup_namespace_not_found() {
        let (mock_service, mut handle) = mock::pair();
        let client = Client::new(mock_service, "default");

        let check_fut = tokio::spawn(async move {
            verify_startup(
                &client,
                "nonexistent-ns",
                "test-instance",
                &ClusterTargetsConfig::default(),
            )
            .await
        });

        let (req, send_resp) = handle.next_request().await.expect("received request");
        assert_eq!(req.uri().path(), "/api/v1/namespaces/nonexistent-ns");
        send_resp.send_response(
            http::Response::builder()
                .status(404)
                .header("Content-Type", "application/json")
                .body(
                    serde_json::to_string(&serde_json::json!({
                        "kind": "Status",
                        "apiVersion": "v1",
                        "code": 404,
                        "message": "namespaces \"nonexistent-ns\" not found"
                    }))
                    .unwrap(),
                )
                .unwrap(),
        );

        let err = check_fut.await.unwrap().unwrap_err();
        assert_eq!(
            err,
            StartupCheckError::NamespaceNotFound("nonexistent-ns".to_string())
        );
    }

    /// Answers every `SelfSubjectAccessReview` with `allow(namespace, resource, verb)` and records
    /// the requested `(namespace, resource, verb)` triples.
    fn spawn_ssar_responder(
        mut handle: mock::Handle<http::Request<kube::client::Body>, http::Response<String>>,
        allow: impl Fn(Option<&str>, &str, &str) -> bool + Send + 'static,
    ) -> tokio::task::JoinHandle<Vec<(Option<String>, String, String)>> {
        tokio::spawn(async move {
            let mut seen = Vec::new();
            while let Some((req, send)) = handle.next_request().await {
                let path = req.uri().path().to_string();
                let body = if path == "/api/v1/namespaces/sandboxes" {
                    serde_json::to_string(&Namespace::default()).unwrap()
                } else {
                    let bytes = http_body_util::BodyExt::collect(req.into_body())
                        .await
                        .unwrap()
                        .to_bytes();
                    let review: SelfSubjectAccessReview = serde_json::from_slice(&bytes).unwrap();
                    let attrs = review.spec.resource_attributes.clone().unwrap();
                    let (ns, resource, verb) = (
                        attrs.namespace.clone(),
                        attrs.resource.clone().unwrap(),
                        attrs.verb.clone().unwrap(),
                    );
                    let allowed = allow(ns.as_deref(), &resource, &verb);
                    seen.push((ns, resource, verb));
                    serde_json::to_string(&SelfSubjectAccessReview {
                        status: Some(SubjectAccessReviewStatus {
                            allowed,
                            ..Default::default()
                        }),
                        ..review
                    })
                    .unwrap()
                };
                send.send_response(
                    http::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                );
            }
            seen
        })
    }

    #[tokio::test]
    async fn startup_checks_watch_permissions_in_every_target_namespace() {
        let (mock_service, handle) = mock::pair();
        let responder = spawn_ssar_responder(handle, |ns, resource, verb| {
            !(ns == Some("nativelink") && resource == "endpointslices" && verb == "watch")
        });
        let client = Client::new(mock_service, "default");
        let targets = ClusterTargetsConfig {
            any_namespace: false,
            namespaces: vec!["forgejo".to_string(), "nativelink".to_string()],
        };

        let err = verify_startup(&client, "sandboxes", "instance", &targets)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            StartupCheckError::MissingPermissions {
                missing: vec![
                    "watch endpointslices.discovery.k8s.io in namespace `nativelink`".to_string()
                ],
            }
        );
        drop(client);
        let seen = responder.await.unwrap();
        assert!(seen.contains(&(
            Some("forgejo".to_string()),
            "services".to_string(),
            "watch".to_string()
        )));
    }

    #[tokio::test]
    async fn startup_checks_cluster_wide_permissions_with_any_namespace() {
        let (mock_service, handle) = mock::pair();
        let responder = spawn_ssar_responder(handle, |ns, _, _| ns.is_some());
        let client = Client::new(mock_service, "default");
        let targets = ClusterTargetsConfig {
            any_namespace: true,
            namespaces: Vec::new(),
        };

        let err = verify_startup(&client, "sandboxes", "instance", &targets)
            .await
            .unwrap_err();
        assert!(
            matches!(&err, StartupCheckError::MissingPermissions { missing } if missing.iter().all(|m| m.ends_with("in namespace `*`"))),
            "{err}"
        );
        drop(client);
        responder.await.unwrap();
    }

    #[tokio::test]
    async fn startup_succeeds_when_everything_is_allowed() {
        let (mock_service, handle) = mock::pair();
        let responder = spawn_ssar_responder(handle, |_, _, _| true);
        let client = Client::new(mock_service, "default");
        let targets = ClusterTargetsConfig {
            any_namespace: false,
            namespaces: vec!["forgejo".to_string()],
        };

        verify_startup(&client, "sandboxes", "instance", &targets)
            .await
            .unwrap();
        drop(client);
        let seen = responder.await.unwrap();
        let expected =
            SANDBOX_PERMISSIONS.len() + INSTANCE_PERMISSIONS.len() + TARGET_PERMISSIONS.len();
        assert_eq!(seen.len(), expected);
    }

    #[tokio::test]
    async fn forbidden_namespace_lookup_is_a_missing_permission() {
        let (mock_service, mut handle) = mock::pair();
        let client = Client::new(mock_service, "default");
        let check = tokio::spawn(async move {
            verify_startup(
                &client,
                "sandboxes",
                "instance",
                &ClusterTargetsConfig::default(),
            )
            .await
        });

        let (_, send_resp) = handle.next_request().await.expect("namespace GET");
        send_resp.send_response(
            http::Response::builder()
                .status(403)
                .header("Content-Type", "application/json")
                .body(
                    serde_json::json!({
                        "kind": "Status", "apiVersion": "v1", "code": 403,
                        "reason": "Forbidden", "message": "forbidden"
                    })
                    .to_string(),
                )
                .unwrap(),
        );

        assert_eq!(
            check.await.unwrap().unwrap_err(),
            StartupCheckError::MissingPermissions {
                missing: vec!["get namespaces/sandboxes (cluster-wide)".to_string()],
            }
        );
    }

    #[tokio::test]
    async fn startup_reports_gaps_in_every_namespace_not_only_the_first() {
        let (mock_service, handle) = mock::pair();
        let responder = spawn_ssar_responder(handle, |ns, resource, verb| {
            let denied = (ns == Some("sandboxes")
                && resource == "persistentvolumeclaims"
                && verb == "create")
                || (ns == Some("instance") && resource == "secrets")
                || (ns == Some("forgejo") && resource == "services" && verb == "list");
            !denied
        });
        let client = Client::new(mock_service, "default");
        let targets = ClusterTargetsConfig {
            any_namespace: false,
            namespaces: vec!["forgejo".to_string()],
        };

        let err = verify_startup(&client, "sandboxes", "instance", &targets)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            StartupCheckError::MissingPermissions {
                missing: vec![
                    "create persistentvolumeclaims in namespace `sandboxes`".to_string(),
                    "get secrets in namespace `instance`".to_string(),
                    "list services in namespace `forgejo`".to_string(),
                ],
            }
        );
        drop(client);
        responder.await.unwrap();
    }
}
