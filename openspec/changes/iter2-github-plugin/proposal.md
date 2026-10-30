# Proposal

## Why

Dark Factory works only with Forgejo, while many repositories live on GitHub, often as private repositories of GitHub Free accounts. `iter2-multi-forge` routes projects by repository host and lets a route set the API host (`api.github.com`) and the `Bearer` scheme; what is missing is a plugin that speaks GitHub's API behind the forge-neutral `darkfactory:integrations@0.1.0` interfaces.

## What Changes

- **GitHub plugin** (`plugins/github`, world `forge`): pull requests (create, find open, get, update) and issues (get, comment) over GitHub's REST API, with the headers GitHub requires (`User-Agent`, `Accept`, `X-GitHub-Api-Version`).
- **Draft modes** (setting `draft-mode`): `native` (default) uses GitHub's draft pull requests, which also exist in private repositories on GitHub Free; where a repository or instance rejects them, the error names `draft-mode: title-prefix`. `title-prefix` marks drafts with a work-in-progress title prefix like the Forgejo plugin, for repositories or GitHub Enterprise Server instances without native drafts. `auto` creates native drafts and falls back to the title prefix, with a warning, where GitHub answers that the repository has no draft pull requests. All forms are recognized when reading and marking ready, so switching the mode later keeps open pull requests working.
- **Native draft state through GraphQL:** GitHub's REST API cannot change the draft state of a pull request; the plugin marks it ready (or back to draft) with the GraphQL mutations on the same API host.
- **Rate limits:** GitHub's primary and secondary rate limits map to `rate-limited` with the wait GitHub asks for. The host no longer retries a `rate-limited` error whose retry-after exceeds its cap and returns it to the caller instead, since GitHub may block integrations that retry before the reset.
- **GitHub Enterprise Server:** `base-url` may carry a path (`https://ghe.example.org/api/v3`), the GraphQL endpoint is derived from it, and `api-version` selects a REST API version the server supports.
- **Default release:** the GitHub plugin ships as its own plugin image (`plugin-github`, like `plugin-forgejo`), and the chart's default values configure `plugins.github` with that image for `github.com` (`apiUrl: https://api.github.com`, `authScheme: bearer`).
- **Docs:** plugin README (settings, API calls, token permissions, error mapping) and a guide for connecting GitHub repositories.

## Capabilities

### New Capabilities
- `github-integration`: pull requests and issues on GitHub behind the forge-neutral interfaces, including draft modes, required request headers, API base URLs and GitHub's error and rate-limit responses.

### Modified Capabilities
- `plugin-host`: a `rate-limited` error with a retry-after above the cap is returned instead of retried.
- `build-pipeline`: the GitHub plugin builds as a component and ships as its own plugin image.
- `helm-deployment`: the default values configure the GitHub plugin image for `github.com`.

## Impact

- New crate `plugins/github` (workspace member, Bazel targets, `MODULE.bazel` manifest); `crates/df-plugins` (retry rule, integration, image component and live tests, test fixtures); `deploy/BUILD.bazel`, `deploy/dev.sh`, `deploy/values-dev.yaml` and `deploy/test_images.sh` (plugin image `plugin-github`); Helm chart (default values, unit tests, README); `dev/config.yaml`, `Justfile`; docs (`crates/df-plugins/docs/configuration.md`, `crates/df-plugins/README.md`, `docs/development.md`).
- New workspace dependency `percent-encoding` (already in `Cargo.lock` through `url`).
- No contract, database or agent protocol change.
- Out of scope: GitHub App authentication (backlog); webhooks (iteration 3, inbound events); merge, branch and check capabilities (iteration 3, task tree and ADR-0033); SSH repository URLs and Git LFS (backlog).
- Crate, plugin, the host's retry change and the plugin image build on `main` and run in parallel to `iter2-multi-forge` (design D10). The default route, the dev configuration and the runs against github.com need its routing (`hosts`, `apiUrl`, `authScheme`); this change is archived after it. `iter2-plugin-distribution` (plugin images, `plugins.<name>` chart entries, shared `PluginEngine`) is archived.
