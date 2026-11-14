# Spec Delta

## ADDED Requirements

### Requirement: Sandbox image pull secrets
Sandbox pods SHALL list the instance's configured image pull secrets (`sandbox.imagePullSecrets`) as pod-level `imagePullSecrets`, in the configured order. They apply to the project image, the runtime image volume and the harness image volume. When none are configured, sandbox pods SHALL have no `imagePullSecrets`.

#### Scenario: Pull secrets configured
- **WHEN** run 1 of `DF-1` is provisioned with `sandbox.imagePullSecrets` set to `corp-mirror` and `ghcr-team`
- **THEN** pod `run-df-1-1` has `imagePullSecrets` `corp-mirror`, `ghcr-team` in that order

#### Scenario: No pull secrets configured
- **WHEN** run 1 of `DF-1` is provisioned and `sandbox.imagePullSecrets` is unset or empty
- **THEN** pod `run-df-1-1` has no `imagePullSecrets`

#### Scenario: Missing Secret
- **WHEN** a configured pull secret does not exist in the sandbox namespace and the project image needs credentials
- **THEN** the pod fails to pull its image and the run ends like any other failed sandbox start; the control plane does not check the Secret beforehand

### Requirement: Valid image pull secret names
The control plane SHALL refuse to start when an entry of `sandbox.imagePullSecrets` is not a valid Kubernetes Secret name (DNS-1123 subdomain: at most 253 characters, lowercase alphanumerics, `-` and `.`, starting and ending with an alphanumeric). The error SHALL name the option and the invalid entry.

#### Scenario: Invalid name
- **WHEN** the instance configuration sets `sandbox.imagePullSecrets` to `Corp_Mirror`
- **THEN** the control plane exits at startup with an error naming `sandbox.imagePullSecrets` and `Corp_Mirror`

#### Scenario: Empty name
- **WHEN** the instance configuration sets `sandbox.imagePullSecrets` to an empty string entry
- **THEN** the control plane exits at startup with an error naming `sandbox.imagePullSecrets`
