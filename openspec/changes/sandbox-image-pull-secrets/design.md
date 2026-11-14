# Design

## Context

See proposal.md for motivation. Current state:

- `render_pod` (`crates/df-k8s/src/render.rs`) builds every sandbox Pod from `RunSandboxSpec` and `ProvisioningConfig`. The Pod pulls three images: the project image (container), the runtime image and the harness image (both image volumes).
- `ProvisioningConfig.sandbox` is `SandboxProvisioningConfig` (`crates/df-core/src/config.rs`), deserialized from the `sandbox:` section of the instance configuration with defaults for every missing key.
- The chart renders that section in `dark-factory.sandboxConfig` (`templates/_helpers.tpl`) from `.Values.sandbox` and `.Values.sandboxNamespace`.
- All sandboxes share one sandbox namespace, separate from the instance namespace. `imagePullSecrets` resolves only Secrets in the Pod's own namespace.
- `design/deployment.md` §2 says the sandbox namespace holds no Secrets; §3 already never grants the control plane Secret access there.

## Goals / Non-Goals

**Goals:**
- One instance-wide list of pull secrets that every sandbox Pod uses.
- No change to rendered Pods when the list is empty.
- No new Kubernetes permission for the control plane.

**Non-Goals:**
- Per-project pull secrets (target: iteration 5 "Configurable projects", `product/iterations/roadmap.md`).
- Per-image pull secrets: Kubernetes has no per-container or per-volume pull secrets (rejected).

## Decisions

### D1: Pod spec, not the sandbox ServiceAccount
The control plane writes `imagePullSecrets` into each Pod spec. Alternative: attach the Secrets to the chart's sandbox ServiceAccount, which needs no Rust change. Rejected: the ServiceAccount admission plugin injects the ServiceAccount's pull secrets only when the Pod lists none. Per-project pull secrets in the Pod spec would then silently drop the instance-wide ones.

### D2: Operator-created Secrets, referenced by name
The chart only references Secrets. Rendering a `dockerconfigjson` from values would put registry credentials into Helm values and release history (rejected). Copying Secrets from the instance namespace would need cross-namespace Secret read/write for the control plane (rejected).

### D3: Separate value, no fallback to `image.pullSecrets`
`image.pullSecrets` names Secrets in the instance namespace for Dark Factory's own images. The sandbox list names Secrets in another namespace for other images; reusing it would reference Secrets that don't exist there.

### D4: Helm shape `[{ name }]`, instance config shape `[name]`
The Helm value matches `image.pullSecrets` and the Kubernetes `LocalObjectReference` shape operators know. The instance configuration takes plain names; the helper maps `.name`. A plain string list in the config keeps the Rust type `Vec<String>`.

### D5: Validate names at config load
`InstanceConfig::from_file` calls a `validate()` on `SandboxProvisioningConfig` (as it does for `scheduling`, `artifacts`, `lifecycleEvents`). It checks the DNS-1123 subdomain rule with a small hand-written function; no dependency needed. An invalid name otherwise fails every Pod create with an API error. Existence of the Secrets is not checked: that needs `get secrets` in the sandbox namespace, contradicting §3.

### D6: Empty list omits the field
`image_pull_secrets: None` when the list is empty, so rendered Pods stay byte-identical to today and existing `render_pod` tests stay valid.

### D7: Future per-project merge rule
When per-project pull secrets land, the Pod list is the instance list followed by the project's entries not already in it. The project list never replaces the instance list: the runtime and harness images are instance-controlled and may depend on instance secrets. Recorded here so the instance key's meaning ("always applied") stays stable.

## Risks / Trade-offs

- [A pull secret in the sandbox namespace is readable by anyone with Secret read there] → Only the kubelet uses it; it is never mounted into a container, sandboxes have no API token and no permissions, and the control plane has no Secret access there. Operators should scope the registry credential to pull-only.
- [A Pod that sets a pull secret for a registry it doesn't need] → Harmless; the kubelet only uses the matching registry entry.
- [Typo in a Secret name that is still DNS-valid] → `ImagePullBackOff`, visible in sandbox events and the run's failure; the how-to lists it under troubleshooting.

## Migration Plan

Additive. Upgrading the chart without setting the value renders the same Pods. Rollback: unset the value or downgrade the chart; operator-created Secrets can stay.
