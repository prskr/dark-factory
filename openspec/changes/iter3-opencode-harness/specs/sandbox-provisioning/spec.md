## MODIFIED Requirements

### Requirement: Harness image from the instance configuration
The harness image mounted for a run SHALL be the image configured for the run's harness (`harness.claudeCode.image` for `claude-code`, `harness.opencode.image` for `opencode`). A run whose harness has no configured image and whose project sets no adapter command SHALL fail provisioning with a message naming the harness.

#### Scenario: Configured image
- **WHEN** `harness.claudeCode.image` is `registry.example.org/claude-harness:0.95.0`
- **THEN** the pod's `/opt/harness/claude-code` volume references that image

#### Scenario: OpenCode image
- **WHEN** a run of an `opencode` persona starts and `harness.opencode.image` is `registry.example.org/opencode-harness:2.2.5`
- **THEN** the pod's `/opt/harness/opencode` volume references that image and the pod has no `/opt/harness/claude-code` volume

#### Scenario: No image configured
- **WHEN** the harness of a run has no configured image and the project sets no adapter command
- **THEN** provisioning fails and the run ends `provisioning_failed` with a message naming the harness
