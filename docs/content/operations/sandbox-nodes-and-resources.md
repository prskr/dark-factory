---
title: Sandbox nodes and resources
weight: 6
---

Where sandbox pods run and how much they may use is set in the Helm values `sandbox.nodeSelector`,
`sandbox.tolerations`, `sandbox.affinity` and `sandbox.resources`. The control plane puts them on
every sandbox pod it creates. The chart also spreads the control plane and the gateway replicas over
nodes.

A change of these values rolls the control plane and the gateway (both hash the whole configuration).
Running sandbox pods keep the spec they were created with; the next run of a task gets the new values.

## Run sandboxes on dedicated nodes

1. Label and taint the nodes that are meant for sandboxes:

   ```bash
   kubectl label node worker-3 dark-factory.io/pool=sandbox
   kubectl taint node worker-3 dark-factory.io/sandbox=true:NoSchedule
   ```

2. Select the label and tolerate the taint in your values:

   ```yaml
   sandbox:
     nodeSelector:
       dark-factory.io/pool: sandbox
     tolerations:
       - key: dark-factory.io/sandbox
         operator: Exists
         effect: NoSchedule
   ```

3. Upgrade the release and start a task. Check where the pod runs:

   ```bash
   kubectl -n dark-factory-sandboxes get pods -o wide
   ```

The selector keeps sandboxes off other nodes, the toleration lets them onto the tainted ones. Without
the taint, other workloads share the nodes; without the selector, sandboxes may land elsewhere.

If no node carries the labels, the pod stays `Pending` and the run ends `connect_timeout` after
`runs.connectTimeoutSeconds` (600 s); see [Troubleshooting](troubleshooting.md). Startup validation
only checks the syntax of labels and tolerations, not that a node has them.

## Spread sandboxes over nodes

`sandbox.affinity` takes a Kubernetes `affinity` (`nodeAffinity`, `podAffinity`, `podAntiAffinity`)
and sets it on every sandbox pod unchanged, next to the node selector and the tolerations. It is
empty by default. A best-effort spread over nodes (replace `homelab` with the release's
`instanceName`):

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
                dark-factory.io/instance: homelab
```

Sandbox pods carry the labels `dark-factory.io/instance`, `dark-factory.io/project`,
`dark-factory.io/persona` and `dark-factory.io/task-key` for selectors. Pod affinity terms match pods
of the sandbox namespace.

- A **preferred** rule never blocks scheduling: on a single node every sandbox runs on it.
- A **required** pod anti-affinity among sandboxes allows one sandbox per node. Runs above the node
  count stay `Pending` and end `connect_timeout`.
- Task volumes are `ReadWriteOnce`. On node-local storage a later run of a task has to go to the
  node of its volume; a rule or selector that excludes that node leaves the pod `Pending`. Before you
  move sandboxes to other nodes, let old tasks end or delete their volumes.

The control plane refuses to start when `sandbox.affinity` has a field Kubernetes does not know (also
with an empty value), a weight outside 1-100, a pod affinity term without `topologyKey`, or a
selector operator that does not fit. The error names `sandbox.affinity` and the path. Other mistakes
reach the API server, which rejects the pod: the run ends `provisioning_failed` with its message.

## Size sandboxes

`sandbox.resources` sets the requests and limits of the agent container. The defaults are in the
chart's `values.yaml`:

```yaml
sandbox:
  resources:
    requests: { cpu: 250m, memory: 512Mi }
    limits: { cpu: "2", memory: 4Gi }
```

The control plane adds nothing of its own: it sets exactly the entries it is given. Quantities are
strings or numbers (`--set sandbox.resources.limits.cpu=4` works without `--set-string`).
Names and quantities are validated at startup (`250m`, `4Gi`, `1.5`, `1e3` are valid; `4 GB`, `-1`
and `abc` are not).

### Change the size

Helm merges an override per key, so you only name what changes:

```yaml
sandbox:
  resources:
    limits:
      memory: 8Gi     # CPU limit and the requests keep their defaults
```

- `null` removes an entry (`limits.cpu: null`) or a whole map (`limits: null`). `{}` removes nothing:
  Helm merges it with the defaults.
- A kustomization over the ConfigMap does not work: the instance configuration is one YAML string,
  and the config checksum is computed before a post-renderer runs, so the pods would not roll.
- A request removed while its limit stays becomes equal to the limit: `requests.cpu: null` reserves
  the full CPU limit.
- A request above its limit is rejected by Kubernetes, and every run ends `provisioning_failed`.
  Raise the limit together with the request.

### Quota

With `sandboxNamespace.create: true` the chart renders a `ResourceQuota` that needs a CPU and a
memory request and limit on every pod. The render fails when `sandbox.resources` lacks one of the
four (the message names the keys). Size the quota (`sandboxNamespace.resourceQuota.*`) for the pod
count times the limits. The chart renders no `LimitRange`.

### Your own LimitRange

With an existing namespace (`sandboxNamespace.create: false`) you can leave the size to a
`LimitRange` you manage: set `sandbox.resources: {requests: null, limits: null}`. The pods then carry
no resources from Dark Factory, and the LimitRange applies its defaults at admission. Its `min`,
`max` and ratio rules also apply to pods that carry resources, and a violation ends the run
`provisioning_failed`.

### What the default reserves

Requests are what the scheduler reserves, limits are the most a pod may use:

| Load | CPU requested | Memory requested | CPU limit sum | Memory limit sum |
| --- | --- | --- | --- | --- |
| One sandbox | 250m | 512Mi | 2 | 4Gi |
| Default concurrency (4 task and 2 chat runs) | 1.5 | 3Gi | 12 | 24Gi |
| `ResourceQuota` default | 16 | 32Gi | 32 | 64Gi |

With Kata, add the `RuntimeClass` overhead (`overhead.podFixed`) to every pod; it counts for
scheduling and for the quota.

### runc and Kata

- **runc:** requests and limits map to the container's cgroup. A container above its memory limit is
  killed (`OOMKilled`, exit code 137). A CPU limit throttles and never kills.
- **Kata:** the pod runs in a microVM that starts with the runtime's base size (stock configuration:
  1 vCPU and 2 GiB) and grows by the container limits: the memory limit on top of the base memory,
  `ceil(CPU limit)` vCPUs. Requests do not change the VM size. Without limits the sandbox gets only
  the base size, so with Kata the limits are the effective size. By default a Kata guest does not
  give memory it has touched back to the host: a sandbox with a 512Mi request and a 4Gi limit can
  hold up to 4Gi on the node after a peak. Kata nodes overcommit more than runc nodes with the same
  values.

The default is Burstable (requests below limits). Memory above the request is overcommitted: under
node memory pressure the kubelet evicts sandboxes first.

## What happens at a limit

| Event | What happens | Run outcome |
| --- | --- | --- |
| Memory limit reached | The kernel kills the container's processes; the pod is `Failed` with `OOMKilled` and exit code 137 | `sandbox_lost`, detail `OOMKilled (exit code 137)`; queued again up to `runs.maxAutoResumes` (default 3), then `failed`. The task volume keeps the work; the harness session of the run is lost |
| Memory limit reached on a node with the kubelet option `singleProcessOOMKill: true` | Only the largest process is killed, usually a build tool or the harness | A killed tool fails inside the turn; a killed harness ends the run `FAILED`; no `OOMKilled` detail |
| CPU limit reached | Throttling | none |
| Node pressure (memory, disk, inodes, PIDs) | The kubelet evicts the pod; sandboxes above their memory request go first under memory pressure | `sandbox_lost`, detail `Evicted` |
| `NoExecute` taint added to the node | The taint manager evicts a pod without a fitting toleration | `sandbox_lost` |
| No node fits selector, tolerations, affinity or resources | The pod stays `Pending` | `connect_timeout` after `runs.connectTimeoutSeconds`, then queued again like above |
| Request above limit, LimitRange or quota rejects the pod | The API server rejects the pod | `provisioning_failed` with the API message; a quota rejection does not count towards the resume limit |

A task that keeps ending `OOMKilled` becomes `failed` after the resume limit: raise
`sandbox.resources.limits.memory` and send the task a new message.

## Spread control plane and gateway replicas

Both Deployments carry a pod anti-affinity against the pods of their own component, release and
revision (`pod-template-hash`) on the same node (`kubernetes.io/hostname`):

```yaml
controlPlane:
  podAntiAffinity: soft   # soft | hard | none
gateway:
  podAntiAffinity: hard
  affinity: {}            # not empty: replaces the preset
```

- `soft` (default) prefers different nodes. A single-node cluster runs every replica on its node.
- `hard` requires them: it needs at least as many eligible nodes as replicas, otherwise a replica
  stays `Pending`.
- `none` renders no anti-affinity.
- `affinity` is rendered as is and replaces the preset, for example to spread over
  `topology.kubernetes.io/zone`. `{}` keeps the preset.

The rule applies per revision. During a rollout a new pod does not count the old revision's pods, so
an old and a new replica may share a node until the old one ends, and the rollout needs room for one
surge pod on an eligible node.

## Reference

| Value | Required | Default | Effect |
| --- | --- | --- | --- |
| `sandbox.nodeSelector` | no | `{}` | Labels (string values) a node must carry to run a sandbox pod. Rendered as `sandbox.nodeSelector` of the instance configuration. |
| `sandbox.tolerations` | no | `[]` | Tolerations (`key`, `operator` `Equal` or `Exists`, `value`, `effect` `NoSchedule`, `PreferNoSchedule` or `NoExecute`, `tolerationSeconds`) of sandbox pods, in order. `Exists` takes no `value`; `tolerationSeconds` needs `NoExecute`. |
| `sandbox.affinity` | no | `{}` | Kubernetes `affinity` of sandbox pods. Top-level keys `nodeAffinity`, `podAffinity`, `podAntiAffinity`. Empty: none. |
| `sandbox.resources.requests`, `sandbox.resources.limits` | no | `cpu: 250m`, `memory: 512Mi` and `cpu: "2"`, `memory: 4Gi` | Requests and limits of the agent container; resource name to quantity. Merged per key, `null` removes. |
| `sandbox.runtimeClassName`, `sandbox.serviceAccountName`, `sandbox.imagePullSecrets` | no | `""`, `dark-factory-sandbox`, `[]` | Runtime class, ServiceAccount and image pull secrets ([how-to](private-sandbox-images.md)) of sandbox pods. |
| `controlPlane.podAntiAffinity`, `gateway.podAntiAffinity` | no | `soft` | `soft`, `hard` or `none`; see above. |
| `controlPlane.affinity`, `gateway.affinity` | no | `{}` | A Kubernetes `affinity` that replaces the preset when not empty. |

The values are rendered one to one under `sandbox.*` of the instance configuration (same keys, see the
[provisioning keys](https://github.com/prskr/dark-factory/blob/main/crates/df-k8s/README.md#provisioning-configuration-keys)).
