# Design: sandbox-scheduling-resources

## Context

See proposal.md for the motivation.

Current state:

- The control plane builds the sandbox pod in `df-k8s` (`render::render_pod`, `render::agent_container`) from `ProvisioningConfig.sandbox`, which is `df_core::config::SandboxProvisioningConfig`, deserialized from the `sandbox` table of the instance configuration. Defaults live in its `Default` impl. The chart renders that table in `_helpers.tpl` (`dark-factory.sandboxConfig`).
- The pod has one container (`agent`, PID 1 is `factory-agent run`, the harness is its child), `restartPolicy: Never`, a memory `emptyDir` at `/run/factory`, the task PVC (`ReadWriteOnce`) at `/data`, and image volumes. It sets no `resources`, `nodeSelector`, `tolerations` or `affinity`.
- Sandbox size today comes only from the chart's `LimitRange` in the sandbox namespace (`sandboxNamespace.limitRange`): default requests `250m` / `512Mi`, default limits `2` / `4Gi`. The `ResourceQuota` (`requests.cpu`, `requests.memory`, `limits.cpu`, `limits.memory`) requires every pod to have all four values, so without the LimitRange every sandbox pod would be rejected. With `sandboxNamespace.create: false` neither object exists, and sandboxes run without any size.
- Up to `runs.maxConcurrent` (4) task runs and `runs.maxConcurrentChats` (2) chat runs are active at the same time, so 6 sandbox pods by default.
- `map_pod_event` (`df-k8s/src/provider.rs`) turns a `Failed` pod into `Gone` with `status.reason`, then `status.message`, then `pod failed`. An OOM kill sets neither pod field, so the detail is `pod failed`.
- Control plane and gateway Deployments: `replicas` default 1, `RollingUpdate` with the default 25 % surge and unavailability (for 1 replica: one extra pod, none unavailable). Both carry `checksum/config` of the whole ConfigMap, so every instance configuration change rolls both. The gateway has a PDB with more than one replica. The control plane elects a leader through a Lease.
- The chart requires Kubernetes 1.36 (`kubeVersion`).

## Goals / Non-Goals

**Goals:**

- Pin sandboxes to a node pool with Helm values only.
- One place for the sandbox size that works with and without the chart-managed namespace, and that profiles can override later without a breaking change.
- Spread control plane and gateway replicas over nodes without breaking single-node clusters.
- Clear failure messages when a limit kills a run.

**Non-Goals:**

- An affinity preset for sandboxes (like `podAntiAffinity: soft` of D8): rejected. Spreading sandboxes is a choice per cluster, so `sandbox.affinity` is empty by default and the docs show the soft spread as an example.
- Sandbox profiles and separate sizes for task and chat runs: roadmap backlog, row "Sandbox pod resources" (reworded at archive).
- Scheduling values for the control plane, gateway and migrate pods beyond `affinity`: roadmap backlog (row added at archive).
- Early `FailedScheduling` detection: roadmap backlog (row added at archive).
- Topology spread constraints for the control plane and the gateway: rejected. Pod anti-affinity is enough for the small replica counts here, and `affinity` covers custom needs.
- `ephemeral-storage` defaults: rejected for now. The resource maps accept any resource name, so an operator can set it; the default does not, because the right size depends on the project image.

## Decisions

### D1. Where the values live: `sandbox.*` for the pod, `sandboxNamespace.*` for the namespace

New keys go under `sandbox` in both the Helm values and the instance configuration: `sandbox.nodeSelector`, `sandbox.tolerations`, `sandbox.affinity`, `sandbox.resources`. The rule: `sandbox.*` is how every sandbox pod looks; `sandboxNamespace.*` is the namespace and the objects in it.

Under that rule three existing Helm values move (**breaking**, confirmed Q4):

| Old Helm value | New Helm value | Instance configuration key (unchanged) |
|---|---|---|
| `sandboxNamespace.runtimeClassName` | `sandbox.runtimeClassName` | `sandbox.runtimeClassName` |
| `sandboxNamespace.imagePullSecrets` | `sandbox.imagePullSecrets` | `sandbox.imagePullSecrets` |
| `sandboxNamespace.serviceAccountName` | `sandbox.serviceAccountName` | `sandbox.serviceAccountName` |

`serviceAccountName` is both a pod setting and the name of the ServiceAccount the chart creates in the namespace. It moves because the pod is what uses it; `sandbox-namespace.yaml` reads `sandbox.serviceAccountName` for the object name. `storageClass` and `pvcSize` stay in `sandboxNamespace`: they size the task volumes, which are namespace objects that outlive every pod. The Rust side and the instance configuration do not change for the moved keys; only the chart's values, schema, templates and tests, the dev and e2e values files, and the docs.

The chart renders the values one to one into the instance configuration, like `sandbox.landlock`. The `sandbox` object of the values schema gets `additionalProperties: false`, so a typo such as `nodeSelectors` fails the render instead of being ignored. The old keys (`sandboxNamespace.runtimeClassName`, `.imagePullSecrets`, `.serviceAccountName`, `.limitRange`) fail the render with a template `fail` that names the new key, for example `sandboxNamespace.runtimeClassName moved to sandbox.runtimeClassName`, so an upgrade with old values fails loudly instead of silently dropping the runtime class. A `false` schema entry was considered; Helm then reports only "false schema" without saying where the key went. `serviceAccountName` and `limitRange` leave the schema's `sandboxNamespace.required` list. `helm upgrade --reuse-values` carries the old keys and fails the same way; the upgrade notes say to use `--values` or `--reset-then-reuse-values` (Risks).

### D2. Instance configuration types stay in `df-core`, without Kubernetes types

`df-core` must not depend on `k8s-openapi` (domain crate, `docs/architecture.md`). New fields of `SandboxProvisioningConfig`:

```rust
/// Labels a node must carry to run a sandbox pod.
pub node_selector: BTreeMap<String, String>,
/// Taints sandbox pods tolerate.
pub tolerations: Vec<SandboxToleration>,
/// Requests and limits of the agent container.
pub resources: SandboxResources,
/// Kubernetes affinity, passed through (D3).
pub affinity: Option<serde_json::Value>,

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxToleration {
    pub key: Option<String>,
    pub operator: TolerationOperator, // Equal (default) | Exists
    pub value: Option<String>,
    pub effect: Option<TaintEffect>,  // NoSchedule | PreferNoSchedule | NoExecute; None: all
    pub toleration_seconds: Option<i64>,
}

#[serde(deny_unknown_fields)]
pub struct SandboxResources {
    #[serde(default)] pub requests: BTreeMap<String, String>,
    #[serde(default)] pub limits: BTreeMap<String, String>,
}
```

`deny_unknown_fields` catches typos (`effects` for `effect`). `df-k8s` maps these into `k8s_openapi` `Toleration` and `ResourceRequirements` (`Quantity`) in `render_pod`. `SandboxResources` has no defaults of its own (empty maps, confirmed Q6): the control plane puts exactly the configured entries on the pod, and an empty map gives the pod no entries of that kind. The defaults live in the chart (D4).

`validate()` (already called at startup for `imagePullSecrets`) gets checks with the error naming the option and entry:

- node selector key: Kubernetes qualified name with optional DNS subdomain prefix; value: at most 63 characters, `[A-Za-z0-9]([-A-Za-z0-9_.]*[A-Za-z0-9])?` or empty;
- toleration: `Exists` has no `value`; an empty `key` needs `Exists`; `tolerationSeconds` needs `NoExecute`;
- resources: name is a qualified name; quantity matches the Kubernetes quantity grammar (one compiled regex, `regex` is already a `df-core` dependency).

Request greater than limit is not checked; the API server rejects the pod and the run ends `provisioning_failed` with the API message. Comparing quantities needs a parser; this is rare and visible.

Alternatives: keeping tolerations as an opaque `serde_json::Value` and deserializing into `k8s_openapi` types in `df-k8s` at startup. Rejected for tolerations: `k8s-openapi` ignores unknown fields, so typos pass silently, and the small type is cheap to mirror. A server-side dry-run pod create at startup was also considered: it would validate everything, but quota admission can fail it on a full namespace, and it adds an API call to startup. Rejected.

### D3. `sandbox.affinity`: optional pass-through, checked at startup

`sandbox.affinity` is optional (default empty) and is applied to every sandbox pod as is, in addition to the node selector and the tolerations. Main use: a soft pod anti-affinity that spreads sandboxes over nodes on a best-effort basis (confirmed Q1):

```yaml
sandbox:
  affinity:
    podAntiAffinity:
      preferredDuringSchedulingIgnoredDuringExecution:
        - weight: 100
          podAffinityTerm:
            topologyKey: kubernetes.io/hostname
            labelSelector:
              matchLabels:
                dark-factory.io/instance: <instanceName>
```

Pod (anti-)affinity terms match pods in the sandbox pod's own namespace by default; the useful sandbox labels are `dark-factory.io/instance`, `dark-factory.io/project`, `dark-factory.io/persona` and `dark-factory.io/task-key` (spec "Sandbox per run").

Shape: mirroring the Kubernetes `Affinity` type (node and pod affinity, preferred and required terms, label and field selectors, namespace selectors) in `df-core` would cost about as much as the rest of this change. Instead:

- `df-core`: `pub affinity: Option<serde_json::Value>` (`serde_json` is already a dependency); `validate()` checks that it is an object. Empty object counts as unset.
- `df-k8s`: `ProvisioningConfig::from_sandbox_config` becomes fallible and parses the value into `k8s_openapi::api::core::v1::Affinity`. `k8s-openapi` skips unknown fields silently, so the parse goes through `serde_ignored`, which calls back with the path of every field the target type ignores, whatever its value (`preferredDuringScheduling: []` and `preferredDuringScheduling: null` alike). Any ignored path fails startup with an error naming `sandbox.affinity` and that path. Known fields set to `null` or to empty lists parse normally. `serde_ignored` (dtolnay, no dependencies besides `serde`) is a new workspace dependency of `df-k8s`; it replaces a serialize-and-compare round trip, which needed null pruning and still lost unknown keys whose value is `null`. The parsed `Affinity` lives in `ProvisioningConfig`; `Affinity` implements only `PartialEq`, so `ProvisioningConfig` drops its `Eq` derive.
- Semantic checks on the parsed value (parsing cannot see them, for example `weight: 101`): preferred term weights 1–100; every pod (anti-)affinity term has a non-empty `topologyKey`; label selector operators `In`, `NotIn`, `Exists`, `DoesNotExist`; node selector operators additionally `Gt`, `Lt`; `In`/`NotIn` with at least one value, `Exists`/`DoesNotExist` without values. These are the mistakes operators make by hand. The full Kubernetes validation (label syntax inside selectors, field selectors, `matchLabelKeys`) is not repeated: such an affinity reaches the API server, which rejects the pod, and the run ends `provisioning_failed` with the API message.
- `render_pod` uses the parsed `Affinity`.
- Chart schema: `sandbox.affinity` allows only `nodeAffinity`, `podAffinity` and `podAntiAffinity` at the top level; deeper checks are left to the control plane (copying the Kubernetes JSON schema into `values.schema.json` is large and goes stale).

Check of "Spread falls back on a single node": `just e2e-local` with the soft spread above (`dark-factory.io/instance: e2e`) as `sandbox.affinity` in `deploy/values-e2e.yaml` (temporary), run with `E2E_TEST_ARGS="--ignored --test-threads=2 coordinator_delegates a_researcher_hands"`. While two tasks ran at once, `kubectl -n dark-factory-sandboxes get pods -o wide` showed `run-dg-2-1` and `run-rs-2-1` both on `df-e2e-control-plane`; both tests passed.

Risks, in the docs: a **required** pod anti-affinity among sandboxes allows one sandbox per node, so runs above the node count stay `Pending` and end `connect_timeout`. Affinity rules also combine with the node pinning of `ReadWriteOnce` task volumes on node-local storage: a later run of a task must go to its volume's node, and a rule that excludes it leaves the pod `Pending`. A preferred rule never blocks scheduling.

### D4. Sandbox resources: explicit in the pod, default equal to today's LimitRange, LimitRange removed

The agent container gets `sandbox.resources`. The defaults live in the chart's `values.yaml` (confirmed Q6):

```yaml
sandbox:
  resources:
    requests: { cpu: 250m, memory: 512Mi }
    limits:   { cpu: "2", memory: 4Gi }
```

The chart always renders both maps into the instance configuration (a missing map as `{}`). The values schema accepts a quantity as a string or a number (`cpu: 2`, `--set sandbox.resources.limits.cpu=4`), and the chart renders every quantity with `quote`, so the instance configuration always holds strings. The control plane adds nothing (D2). Reasons:

- Transparency: an operator sees every value that reaches the pod in `values.yaml` and in the rendered ConfigMap; there is no hidden second default in the binary.
- Running without Dark Factory limits stays possible, for example with an operator-managed LimitRange: no entries in the instance configuration means no entries on the pod.

Overrides follow Helm's value merging, like every other map in the chart (`controlPlane.resources`): `sandbox.resources.limits.memory: 8Gi` keeps the default CPU limit; an entry or a whole map is removed with `null` (`limits.cpu: null`, `limits: null`). An empty map (`limits: {}`) does not remove the defaults, because Helm merges it with them; the `values.yaml` comment and the docs say so. A post-renderer (kustomization) is not a good way to change resources: the instance configuration is one YAML string in the ConfigMap's `data.config.yaml`, so a patch must replace the whole string, and `checksum/config` is computed before the post-renderer runs, so the change does not roll the control plane. The docs recommend `null` instead.

Two Kubernetes rules that per-key overrides make easy to hit, both documented (tasks 5.1, 5.2):
- Removing a request while its limit stays makes the API server set the request equal to the limit (`requests.cpu: null` → the pod reserves 2 CPU).
- A request above its limit (`requests.memory: 8Gi` with the default 4Gi limit) is rejected by the API server; every run ends `provisioning_failed`. Raise the limit together with the request. Tests (task 4.2): default renders the four values; `limits.memory: 8Gi` renders `{cpu: "2", memory: 8Gi}`; `limits.cpu: null` renders `{memory: 4Gi}`; `{requests: null, limits: null}` renders two empty maps.

Alternative: defaults only in `df-core`, `values.yaml` empty, whole-map replacement. Rejected by Peter Kurfer: the values an operator gets are not visible in the chart, and "no resources" could not be configured, because a missing map would fall back to the binary's defaults.

These are exactly the values the chart's LimitRange applies today, so with `sandboxNamespace.create: true` an upgrade does not change the size of any sandbox. With `create: false` it does: those sandboxes had no Dark Factory size so far (only what an operator's own LimitRange gives them) and now get the chart defaults. On runc they go from no memory limit to 4Gi, so a heavy build can now end `OOMKilled`; on Kata they go from the base VM size (2 GiB with the stock configuration) to base + 4Gi. An operator-managed LimitRange stops filling in its defaults, and its `max` or `maxLimitRequestRatio` can reject the new values. Keeping the old behaviour takes `sandbox.resources: {requests: null, limits: null}` (D9, migration plan). The LimitRange and `sandboxNamespace.limitRange` are removed (**breaking**; the old key fails the render with a migration message, D1). Reasons:

- One source of truth. With both, the pod value wins over the LimitRange default and the LimitRange values become dead settings that operators still have to read.
- It works with `sandboxNamespace.create: false`: the size travels with the pod, not with the namespace.
- Profiles can override it later (D5).

The quota check: with `sandboxNamespace.create: true` the chart renders the ResourceQuota with `requests.cpu`, `requests.memory`, `limits.cpu` and `limits.memory`. A pod without one of them is rejected by quota admission. The chart fails the render when `sandbox.resources` lacks a `cpu` or `memory` request or limit (`fail` in `sandbox-namespace.yaml`, message naming the missing keys and the quota), instead of every run ending `provisioning_failed`. The defaults pass. With `create: false` there is no check: an operator-managed LimitRange then fills missing values, and its min/max rules can reject pods (documented).

What the default reserves on a small cluster (requests are what the scheduler reserves; limits are the most a pod may use):

| Load | CPU requested | Memory requested | CPU limit sum | Memory limit sum |
|---|---|---|---|---|
| One sandbox | 250m | 512Mi | 2 | 4Gi |
| Default concurrency (4 task + 2 chat runs) | 1.5 | 3Gi | 12 | 24Gi |
| ResourceQuota default (for comparison) | 16 | 32Gi | 32 | 64Gi |

Under Kata, add the RuntimeClass `overhead.podFixed` to every pod; it counts for scheduling and for the quota (for example 250m / 160Mi with the kata-deploy defaults, so 1.5 CPU / ~1Gi more for 6 pods).

The default is Burstable (requests below limits). Memory above the request is overcommitted: under node memory pressure the kubelet evicts sandboxes first (they exceed their requests), the run ends `sandbox_lost` with `Evicted`. Q2 asks whether the memory request should be higher.

CPU limit: a CPU limit throttles; it never kills. It stays in the default because (a) the ResourceQuota requires `limits.cpu` and (b) under Kata a CPU limit is what gives the VM more vCPUs (below). Without a CPU limit a runc sandbox may use idle CPU of the node, a Kata sandbox stays at the VM's base vCPUs.

Runtime differences:

- **runc:** requests and limits map to the container cgroup (`cpu.weight`, `cpu.max`, `memory.max`). On cgroup v2, since Kubernetes 1.28, the kubelet sets `memory.oom.group`, so an OOM kills every process of the container (unless the node sets the kubelet option `singleProcessOOMKill: true`, added in 1.32). PID 1 is `factory-agent`, so the container ends with `OOMKilled`, exit code 137.
- **Kata:** the pod runs in a microVM. The VM starts with the runtime's base size (`default_vcpus`, `default_memory`; 1 vCPU and 2 GiB with the stock configuration) and grows by the container limits: the memory limit is hot-plugged on top of the base memory, the CPU limit adds `ceil(limit)` vCPUs. Requests do not change the VM size. Without limits the container gets only the base VM size, so with Kata `limits` are the effective sandbox size. The memory `emptyDir` at `/run/factory` uses guest memory and counts against the limit in both runtimes (it holds only a few certificates and tokens). By default a Kata guest does not give memory it has touched back to the host, so a Burstable sandbox (request 512Mi, limit 4Gi) can keep up to its limit on the node after a peak; Kata nodes overcommit much more than runc nodes with the same values. The docs say so (task 5.1); the default size stays (Q2). How Kata reports a guest OOM to the kubelet (`OOMKilled` or a plain error exit) is checked in task 1.1; the spec scenario "Container killed for memory" is written for runc.

Spike result (task 1.1), runc only. Cluster: the e2e kind cluster (Kubernetes 1.37, runc, cgroup v2, node with 8 CPUs and 16 GB memory). Pod `agent` container from `python:3.13-alpine`, requests `250m`/`512Mi`, limits `cpu: 2`, `memory: 1Gi`, command `sh -c 'nproc; grep MemTotal /proc/meminfo; cat /sys/fs/cgroup/memory.max /sys/fs/cgroup/cpu.max; python -c "b = b\"x\" * (2 * 1024**3)"; echo python exited $?'`:

- In the container: `nproc` 8 and `MemTotal` 16368076 kB (the node, runc does not shrink the view), `memory.max` 1073741824, `cpu.max` `200000 100000`.
- Pod phase `Failed`, no `status.reason`, no `status.message`; container `agent` terminated with `reason: OOMKilled`, `exitCode: 137`. The shell (PID 1) was killed together with its python child (`python exited` was never printed), so the whole container is killed, as D4 states for `memory.oom.group`.
- This is the case `map_pod_event` reads (D6): the detail becomes `OOMKilled (exit code 137)`.

Kata was not checked: no cluster with a Kata `RuntimeClass` was reachable (the only other cluster in the environment offers `gvisor`, not Kata, and is not a test cluster). The Kata statements above (VM size grows with the limits, guest memory is kept, how a guest OOM reaches the kubelet) stay documentation-based and are unverified. The troubleshooting docs describe only the runc observation. To close the gap, repeat the pod above with `runtimeClassName: kata` and record `nproc`, `MemTotal`, the container state and the pod phase; add a Kata scenario to "Sandbox events" if the reason is not `OOMKilled`.

### D5. Ready for sandbox profiles

The later profile change adds named profiles, for example:

```yaml
sandbox:
  resources: { ... }          # this change: the base for every sandbox pod
  profiles:                   # later change
    large:
      resources: { limits: { memory: 16Gi } }
```

and a way to select one (per project, per run kind). To keep that non-breaking:

- `sandbox.resources` keeps its name, shape and meaning: the base when no profile applies. A profile sets the same `{requests, limits}` shape; how it combines with the base (per key like Helm, or per map) is decided in the profile change. Both work with the base of this change.
- `render_pod` takes the resolved `SandboxResources` through one function (`effective_resources(spec, config)`, today returning `config.sandbox.resources`). The profile change extends `RunSandboxSpec` with the chosen profile and changes only that function.
- The chart's quota check (D4) checks the base. Profiles will need the same check per profile; noted in the backlog row.

The same applies to scheduling: profiles may later carry their own `nodeSelector`, `tolerations` and `affinity` (GPU pool); `sandbox.nodeSelector`, `sandbox.tolerations` and `sandbox.affinity` stay the base.

### D6. Out-of-memory reason in `Gone`

`map_pod_event` for a `Failed` pod: when the pod has no `status.reason` (`Evicted` is a pod reason and stays first), take the terminated state of the `agent` container: `<reason> (exit code <n>)`, for example `OOMKilled (exit code 137)`. Then `status.message`, then `pod failed` as today. This is the existing `sandbox_lost` path, so the run detail and the troubleshooting docs pick it up. No new run outcome: an OOM-killed sandbox is a lost sandbox, and the automatic resume (D7) is the same.

### D7. Failure behaviour of limits

| Event | What happens | Run outcome |
|---|---|---|
| Container reaches the memory limit (runc, default kubelet) | Kernel kills all processes of the container; pod `Failed`, container `OOMKilled`, exit 137 | `sandbox_lost`, detail `OOMKilled (exit code 137)`; queued again with back-off up to `runs.maxAutoResumes` (3), then `failed`. The task volume keeps the work; uncommitted files stay, the harness session of the run is lost like on any lost sandbox |
| Same, node with `singleProcessOOMKill: true` | Kernel kills the largest process, usually the harness or a build tool; `factory-agent` survives | A killed tool fails inside the harness turn; a killed harness ends the run as a harness exit (`FAILED`); no `OOMKilled` detail |
| CPU limit reached | Throttling; the run gets slower, nothing is killed | none |
| Node pressure (memory, disk, inodes or PIDs); under memory pressure sandboxes above their memory request go first | Kubelet evicts the pod; the pod message names the resource | `sandbox_lost`, detail `Evicted` (already reported); the remedy depends on the resource in the pod message and the node conditions |
| `NoExecute` taint added to the node, toleration without or with expired `tolerationSeconds` | Taint manager evicts the pod | `sandbox_lost` |
| No node matches selector and tolerations, or not enough allocatable resources | Pod stays `Pending` | `connect_timeout` after `runs.connectTimeoutSeconds` (600 s), then automatic resume like above |
| Request above limit, or rejected by an operator LimitRange or the quota | API rejects the pod | `provisioning_failed` with the API message (a quota rejection does not count towards the resume limit, as today) |

Repeated OOM kills of the same task end it `failed` after the resume limit; the docs tell operators to raise `sandbox.resources.limits.memory` and send the task a new message.

### D8. Anti-affinity of the control plane and the gateway

Values per component (`controlPlane`, `gateway`):

```yaml
podAntiAffinity: soft   # soft | hard | none
affinity: {}            # non-empty: rendered as is, replaces the preset
```

Rendered preset (`soft`; `hard` uses `requiredDuringSchedulingIgnoredDuringExecution` with the same term and no weight):

```yaml
affinity:
  podAntiAffinity:
    preferredDuringSchedulingIgnoredDuringExecution:
      - weight: 100
        podAffinityTerm:
          topologyKey: kubernetes.io/hostname
          labelSelector:
            matchLabels: <component selector labels>  # name, instance, component
          matchLabelKeys: [pod-template-hash]
```

`matchLabelKeys: [pod-template-hash]` (GA well before the chart's minimum Kubernetes 1.36) limits the term to pods of the same ReplicaSet revision. In steady state all replicas share the hash, so they spread; during a rollout the new pods ignore the old ones, so a surge pod may start next to an old pod. Trade-off: while a rollout runs, a node failure can take down an old and a new replica together; the window ends when the old pods are gone. The rollout still needs room for one surge pod on some eligible node (`maxSurge` 1, `maxUnavailable` 0 for small replica counts); without it the surge pod stays `Pending`, as with any Deployment.

- **What it spreads across:** nodes (`kubernetes.io/hostname`, set on every node). Zones (`topology.kubernetes.io/zone`) are not set on many small and on-prem clusters; there a zone rule does nothing useful. Zone spreading is one `affinity` override away (spec scenario "Zone spreading").
- **Against which pods:** only pods of the same component of the same release (the Deployment's selector labels). The control plane and the gateway may share a node; two releases do not influence each other.
- **Soft by default:** with a preferred rule, a single-node cluster schedules every replica on its only node, and a rolling update's surge pod can still start. A hard rule leaves a replica `Pending` when there are fewer eligible nodes than replicas; `hard` is documented as "needs at least as many eligible nodes as replicas". Thanks to `matchLabelKeys`, a hard rule does not block rolling updates when the node count equals the replica count: the surge pod does not count the old pods.
- **Rendered for 1 replica too:** the pod template does not depend on the replica count, so changing `replicas` never rolls the pods, and the surge pod of a rollout prefers another node.

The helper `dark-factory.podAffinity` (args: root context, component values, selector labels template name) renders the block for both Deployments.

Checks in the single-node e2e kind cluster (Kubernetes 1.37):

- "Single-node cluster": `just e2e-local` with `gateway.replicas: 2` added to `deploy/values-e2e.yaml` (temporary), default `gateway.podAntiAffinity: soft`. `kubectl -n dark-factory get pods -o wide` showed both gateway pods `Running` on `df-e2e-control-plane`; the Deployment reported 2 of 2 ready.
- "Hard rule does not block a rollout": not checked. It needs a cluster with two nodes, and only the single-node kind cluster was available.

Alternative: one `affinity` value only, with the default anti-affinity in `values.yaml`. Rejected: an operator who wants to add one node affinity term must then copy the whole default, and an empty `{}` override would silently remove the spread.

### D9. Upgrade impact

| Change | What restarts | Brief unavailability |
|---|---|---|
| Upgrading to this chart version (anti-affinity added, instance configuration changed) | Control plane and gateway Deployments roll once (new pod template and new `checksum/config`). The migrate job runs as usual. The LimitRange is deleted; sandbox pods the old control plane creates until the new one leads are rejected by the quota (migration plan, step 2). | Control plane: surge pod first, leader Lease passes to it; the web UI and the agent port drop connections during the switch, sandboxes reconnect. Gateway: the connections of a terminating gateway pod (sandbox egress, Git proxy) are cut, also with 2+ replicas; the PDB does not apply to rolling updates. With 1 replica, new connections fail until the new pod is ready; with 2+ replicas, new connections go to a ready replica. |
| Same upgrade, release with `sandboxNamespace.create: false` | As above | New sandboxes get the chart default size instead of none (D4): new `OOMKilled` runs on runc are possible, and an operator LimitRange's `max` can reject pods. `sandbox.resources: {requests: null, limits: null}` keeps the old behaviour. |
| Changing `sandbox.nodeSelector`, `sandbox.tolerations`, `sandbox.affinity`, `sandbox.resources` or `sandbox.runtimeClassName` | ConfigMap changes, so control plane and gateway roll (both hash the whole ConfigMap). Running sandbox pods are not changed (pod scheduling fields and resources are immutable here); new values apply to pods created afterwards. | Same as above. Active runs continue on their old pods. |
| Changing `podAntiAffinity` or `affinity` | Only the changed component rolls. | Same as above for that component. |
| Moving sandboxes to other nodes with node-local or zonal task volumes | Next run of an existing task needs its volume's node or zone | If the selector excludes it, the pod stays `Pending` (volume node affinity conflict) and the run ends `connect_timeout`. Documented: drain old tasks first or delete their volumes. |

Making the gateway skip config-only rollouts (hash only the keys it reads) is rejected (Q5): with two or more gateway replicas a rollout cuts only the connections of the replaced pod, and new connections keep working.

Renaming `sandboxNamespace.serviceAccountName` to `sandbox.serviceAccountName` with the same value renders the same ServiceAccount; nothing restarts because of it. A changed name creates a new ServiceAccount and deletes the old one; running sandbox pods keep working (they mount no token), new pods use the new name.

## Risks / Trade-offs

- [Memory limit kills long agent work] → Default equals today's effective 4Gi, so nothing gets worse; the `OOMKilled` detail and a troubleshooting row tell operators what to raise.
- [Operators with `sandboxNamespace.create: true` who tuned `sandboxNamespace.limitRange`] → The old key fails the render with a migration message, so the upgrade fails loudly; the upgrade notes give the mapping (`defaultRequestCpu` → `sandbox.resources.requests.cpu`, …).
- [Old values files with `sandboxNamespace.runtimeClassName` etc.] → The old keys fail the render with a message naming the new key; silently dropping `runtimeClassName: kata` would run untrusted code without the microVM, so a loud failure is required.
- [`helm upgrade --reuse-values`] → Helm then reuses the last release's values and does not apply the new chart's defaults: the old keys are present (render fails with the migration message), and `sandbox.resources`, `controlPlane.podAntiAffinity` and the other new keys are missing. The upgrade notes say to upgrade with the values file (`--values`) or with `--reset-then-reuse-values` (Helm 3.14 or newer), which applies the new defaults first. The templates cannot fill in missing maps without duplicating the defaults of `values.yaml`, so the upgrade notes are the safeguard; a missing `sandbox.resources` also fails the quota check, so the mistake does not pass silently.
- [`sandbox.affinity` with fields `k8s-openapi` does not know] → `serde_ignored` reports every ignored field path, also with an empty or `null` value (D3); the tests in task 3.2 cover such inputs and valid `null` and empty lists in known fields.
- [An affinity error that the startup checks do not cover] → The API server rejects the pod and the run ends `provisioning_failed` with its message; the docs name this.
- [Required pod anti-affinity among sandboxes caps runs at the node count] → Docs show the soft form and state the effect of the required form.
- [Kata OOM reporting differs from runc] → Task 1.1 checks it; the docs describe the observed behaviour.
- [Hard anti-affinity leaves replicas `Pending` on too few nodes] → Soft is the default; the docs state the node requirement for `hard`, that the rule applies per revision (old and new pods may share a node during a rollout), and that a rollout needs room for one surge pod.
- [Node selector typo leaves every run `Pending` for 10 minutes] → Startup validation catches syntax only, not a label no node has. The troubleshooting row points to `kubectl get events` (`FailedScheduling`); early detection is in the backlog.

## Migration Plan

1. Release notes: `sandboxNamespace.limitRange` is gone; move custom values to `sandbox.resources` (`defaultRequestCpu` → `requests.cpu`, `defaultRequestMemory` → `requests.memory`, `defaultCpu` → `limits.cpu`, `defaultMemory` → `limits.memory`). Rename `sandboxNamespace.runtimeClassName`, `sandboxNamespace.imagePullSecrets` and `sandboxNamespace.serviceAccountName` to `sandbox.*` (D1). Without the rename the render fails. Releases with `sandboxNamespace.create: false`: sandboxes now get the chart defaults (D4); set `sandbox.resources: {requests: null, limits: null}` to keep running without Dark Factory resources. In this repository `deploy/values-e2e.yaml` (`create: true`) moves its `limitRange` block to `sandbox.resources` (`requests: {cpu: 100m, memory: 128Mi}`, `limits: {cpu: "1", memory: 1Gi}`). `deploy/values-dev.yaml` has `create: false`, so its `limitRange` block never applied: it is deleted, and its sandboxes take the chart defaults, which gives its Kata sandboxes more memory than the base VM they get today (Q7).
2. `helm upgrade`: the control plane and the gateway roll once; existing sandboxes keep running. Upgrade window: Helm deletes the LimitRange while the old control plane still holds the leader Lease (until the new pod is ready, plus up to the lease expiry). Sandbox pods the old control plane creates in that window have no `resources`, and the ResourceQuota rejects them (`must specify limits.cpu…`). That message is not an `exceeded quota` rejection, so the run ends `provisioning_failed`, is queued again and counts towards `runs.maxAutoResumes`. Usually that is at most one attempt per affected task. To avoid it, upgrade when no run is starting (for example with no queued tasks). The upgrade notes say so (task 5.4).
3. Rollback: `helm rollback` restores the LimitRange; sandbox pods created by the new control plane keep their explicit resources until they end.

## Confirmed decisions

Decided by Peter Kurfer on 2026-11-23:

- **Q1 (D3).** `sandbox.affinity` is part of this change, optional, empty by default.
- **Q2 (D4).** Default size stays today's LimitRange values (requests 250m / 512Mi, limits 2 / 4Gi).
- **Q3 (D4).** The LimitRange is removed. Nothing besides the control plane should create pods in the sandbox namespace, so a namespace fallback has no value.
- **Q4 (D1).** `runtimeClassName`, `imagePullSecrets` and `serviceAccountName` move from `sandboxNamespace.*` to `sandbox.*` in this change.
- **Q6 (D2, D4).** The resource defaults live in the chart's `values.yaml`, not in the control plane; overrides follow Helm's merging (`null` removes an entry). Without configured resources the control plane sets none.
- **Q7 (migration plan).** `deploy/values-dev.yaml` (`create: false`, Kata) takes the chart defaults instead of keeping "no limits". With Kata, no limit means only the base VM, so the defaults give dev sandboxes more memory, not less.
- **Q5 (D9).** The gateway keeps rolling on every ConfigMap change: rejected as follow-up. With two or more gateway replicas a rollout cuts only the connections of the replaced pod; new connections keep working.
