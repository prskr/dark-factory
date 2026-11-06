//! Live check that a run's `NetworkPolicy` lets a sandbox reach a cluster target through its
//! cluster IP and nothing else in the cluster (design D5).

use super::*;
use kube::api::PostParams;
use kube::runtime::wait::{await_condition, conditions};
use uuid::Uuid;

/// Image of the target servers and the probes (`httpd`, `nc -z -w`).
const PROBE_IMAGE: &str = "busybox:1.37";
/// Port the target servers listen on, also the Service port.
const PORT: u16 = 8080;
/// Probe exit code base: bit 0 is set if the target was reachable, bit 1 if the other Service was.
const EXIT_BASE: i32 = 10;
const WAIT: std::time::Duration = std::time::Duration::from_secs(180);

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A pod with the run's labels connects to two cluster IPs: the cluster target of its policy and a
/// second Service in the same namespace that is no target. Only the target is reachable. A control
/// probe outside the sandbox namespace reaches both, so the blocked connection is the policy's
/// doing. The probes report through their exit code (no exec).
///
/// Creates a namespace `df-live-reach-<id>` with two server pods and Services, and a pod and a
/// `NetworkPolicy` `df-live-reach-<id>` in the sandbox namespace. Everything is deleted again at the
/// end, also if an assertion failed.
#[tokio::test]
#[ignore = "requires a live cluster (DF_LIVE_* variables, see README)"]
async fn sandbox_reaches_the_target_cluster_ip_and_not_another_service() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let client = Client::try_default().await.expect("cluster connection");
    let config = crate::test_support::live_config(&client).await;
    let id = Uuid::now_v7().simple().to_string();
    let name = format!("df-live-reach-{}", &id[id.len() - 8..]);

    let outcome = probe_both_services(&client, &config, &name).await;

    let sandbox_ns = &config.sandbox.namespace;
    // Best-effort cleanup.
    bounded(
        Api::<Pod>::namespaced(client.clone(), sandbox_ns).delete(&name, &DeleteParams::default()),
    )
    .await
    .ok();
    bounded(
        Api::<NetworkPolicy>::namespaced(client.clone(), sandbox_ns)
            .delete(&name, &DeleteParams::default()),
    )
    .await
    .ok();
    bounded(
        Api::<k8s_openapi::api::core::v1::Namespace>::all(client.clone())
            .delete(&name, &DeleteParams::default()),
    )
    .await
    .ok();

    let (sandbox_exit, control_exit) = outcome.expect("probes ran");
    assert_eq!(
        sandbox_exit,
        EXIT_BASE + 1,
        "sandbox probe: only the target is reachable"
    );
    assert_eq!(
        control_exit,
        EXIT_BASE + 3,
        "control probe: both Services are reachable without the run's policy"
    );
}

/// Sets up the servers, the policy and both probes; returns the exit codes of the sandbox probe
/// and the control probe.
async fn probe_both_services(
    client: &Client,
    config: &ProvisioningConfig,
    name: &str,
) -> TestResult<(i32, i32)> {
    let namespaces: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(client.clone());
    bounded(namespaces.create(
        &PostParams::default(),
        &serde_json::from_value(serde_json::json!({"metadata": {"name": name}}))?,
    ))
    .await?;
    let target_ip = serve(client, name, "target").await?;
    let other_ip = serve(client, name, "other").await?;

    let target = resolve_cluster_target(client, name, "target", &[], false).await?;
    let run_id = RunId::new();
    let mut policy =
        render_network_policy(run_id, &"DF-1".parse()?, name, "unused", &[target], config);
    // The probe pod does not exist yet: applying the policy first means the probe never runs
    // without it. It is deleted explicitly instead of through the owner.
    policy.metadata.owner_references = None;
    bounded(
        Api::<NetworkPolicy>::namespaced(client.clone(), &config.sandbox.namespace)
            .create(&PostParams::default(), &policy),
    )
    .await?;

    let labels = serde_json::json!({
        "app.kubernetes.io/managed-by": "dark-factory",
        "dark-factory.io/instance": config.instance_name,
        "dark-factory.io/run-id": run_id.to_string(),
    });
    let sandbox_exit = run_probe(
        client,
        &config.sandbox.namespace,
        name,
        &labels,
        (target_ip, other_ip),
    )
    .await?;
    let control_exit = run_probe(
        client,
        name,
        "control-probe",
        &serde_json::json!({}),
        (target_ip, other_ip),
    )
    .await?;
    Ok((sandbox_exit, control_exit))
}

/// Starts an HTTP server pod labelled `app=<service>` and a Service of that name in `namespace`;
/// returns the Service's cluster IP once the pod runs.
async fn serve(client: &Client, namespace: &str, service: &str) -> TestResult<IpAddr> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod = pod_manifest(
        service,
        &serde_json::json!({ "app": service }),
        &["httpd", "-f", "-p", &PORT.to_string(), "-h", "/tmp"],
    );
    bounded(pods.create(&PostParams::default(), &serde_json::from_value(pod)?)).await?;
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let svc = bounded(services.create(
        &PostParams::default(),
        &serde_json::from_value(serde_json::json!({
            "metadata": { "name": service },
            "spec": {
                "selector": { "app": service },
                "ports": [{ "port": PORT, "targetPort": PORT }],
            },
        }))?,
    ))
    .await?;
    tokio::time::timeout(
        WAIT,
        await_condition(pods, service, conditions::is_pod_running()),
    )
    .await??;
    let ip = svc
        .spec
        .and_then(|spec| spec.cluster_ip)
        .ok_or_else(|| format!("Service {service} has no ClusterIP"))?;
    Ok(ip.parse()?)
}

/// Runs a probe pod against `(target, other)` and returns its exit code.
async fn run_probe(
    client: &Client,
    namespace: &str,
    name: &str,
    labels: &serde_json::Value,
    (target, other): (IpAddr, IpAddr),
) -> TestResult<i32> {
    // Each address gets a few attempts: the policy may take a moment to be programmed.
    let script = format!(
        "reach() {{ for i in 1 2 3 4 5; do nc -z -w 3 \"$1\" {PORT} && return 0; sleep 1; done; \
         return 1; }}; r={EXIT_BASE}; reach {target} && r=$((r+1)); reach {other} && r=$((r+2)); \
         exit $r"
    );
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    bounded(pods.create(
        &PostParams::default(),
        &serde_json::from_value(pod_manifest(name, labels, &["sh", "-c", &script]))?,
    ))
    .await?;
    let pod = tokio::time::timeout(
        WAIT,
        await_condition(pods, name, |pod: Option<&Pod>| {
            terminated_exit_code(pod).is_some()
        }),
    )
    .await??;
    terminated_exit_code(pod.as_ref())
        .ok_or_else(|| format!("probe {namespace}/{name} has no exit code").into())
}

/// Runs a Kubernetes API call under [`K8S_API_TIMEOUT`].
async fn bounded<T>(call: impl Future<Output = Result<T, kube::Error>>) -> TestResult<T> {
    Ok(tokio::time::timeout(K8S_API_TIMEOUT, call).await??)
}

fn terminated_exit_code(pod: Option<&Pod>) -> Option<i32> {
    pod?.status
        .as_ref()?
        .container_statuses
        .as_ref()?
        .first()?
        .state
        .as_ref()?
        .terminated
        .as_ref()
        .map(|terminated| terminated.exit_code)
}

/// A single-container busybox pod that satisfies the `restricted` Pod Security Standard.
fn pod_manifest(name: &str, labels: &serde_json::Value, command: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "metadata": { "name": name, "labels": labels },
        "spec": {
            "restartPolicy": "Never",
            "automountServiceAccountToken": false,
            "securityContext": {
                "runAsNonRoot": true,
                "runAsUser": 1000,
                "runAsGroup": 1000,
                "seccompProfile": { "type": "RuntimeDefault" },
            },
            "containers": [{
                "name": "probe",
                "image": PROBE_IMAGE,
                "command": command,
                "securityContext": {
                    "allowPrivilegeEscalation": false,
                    "capabilities": { "drop": ["ALL"] },
                },
            }],
        },
    })
}
