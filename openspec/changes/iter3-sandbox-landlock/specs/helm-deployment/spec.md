## ADDED Requirements

### Requirement: Sandbox Landlock value
The chart SHALL expose `sandbox.landlock` (`enforce` or `disabled`, default `enforce`) and render it as `sandbox.landlock` in the instance config. The values schema SHALL reject any other value.

#### Scenario: Default
- **WHEN** the chart is rendered without `sandbox.landlock`
- **THEN** the instance config contains `sandbox.landlock: enforce`

#### Scenario: Opt-out
- **WHEN** the chart is rendered with `sandbox.landlock: disabled`
- **THEN** the instance config contains `sandbox.landlock: disabled`

#### Scenario: Invalid value rejected
- **WHEN** the chart is rendered with `sandbox.landlock: best-effort`
- **THEN** rendering fails schema validation
