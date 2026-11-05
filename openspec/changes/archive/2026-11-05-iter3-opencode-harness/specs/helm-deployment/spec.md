## MODIFIED Requirements

### Requirement: Harness image values
The chart SHALL offer `image.harness.claudeCode.repository`, `.tag` and `.digest` and `image.harness.opencode.repository`, `.tag` and `.digest` (digest wins over tag; an empty tag and an empty digest fail the render, since the tag is the harness version, not the chart version) and render the resulting references as `harness.claudeCode.image` and `harness.opencode.image` of the instance configuration.

#### Scenario: Tag
- **WHEN** the repository is `registry.example.org/claude-harness` and the tag `0.95.0`
- **THEN** the instance configuration has `image: "registry.example.org/claude-harness:0.95.0"`

#### Scenario: Digest
- **WHEN** a digest `sha256:…` is set
- **THEN** the reference is `registry.example.org/claude-harness@sha256:…`

#### Scenario: OpenCode image
- **WHEN** `image.harness.opencode.repository` is `registry.example.org/opencode-harness` and the tag `2.2.5`
- **THEN** the instance configuration has `harness.opencode.image: "registry.example.org/opencode-harness:2.2.5"`

#### Scenario: Missing harness version
- **WHEN** `image.harness.opencode.tag` and `image.harness.opencode.digest` are empty
- **THEN** the render fails with a message naming `image.harness.opencode.tag`
