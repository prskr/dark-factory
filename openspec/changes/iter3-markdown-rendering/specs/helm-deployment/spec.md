# Spec Delta

## ADDED Requirements

### Requirement: Code theme values
The chart SHALL expose `web.codeTheme.light` (default `InspiredGitHub`) and `web.codeTheme.dark` (default `base16-ocean.dark`) and render them as `web.codeTheme.light` and `web.codeTheme.dark` of the instance configuration. The values schema SHALL reject any name outside the bundled themes: `InspiredGitHub`, `Solarized (light)`, `Solarized (dark)`, `base16-ocean.light`, `base16-ocean.dark`, `base16-eighties.dark`, `base16-mocha.dark`.

#### Scenario: Defaults
- **WHEN** a release is rendered without code theme values
- **THEN** the instance configuration has `codeTheme` with `light: "InspiredGitHub"` and `dark: "base16-ocean.dark"` under `web`

#### Scenario: Theme name with spaces
- **WHEN** `web.codeTheme.dark` is `Solarized (dark)`
- **THEN** the instance configuration has `dark: "Solarized (dark)"`, quoted

#### Scenario: Unknown theme
- **WHEN** `web.codeTheme.light` is `Monokai`
- **THEN** rendering fails with a schema error
