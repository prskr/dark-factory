# Proposal

## Why

Projects need concrete guidance for building the sandbox image in which their agents work. The
current project setup page names only the minimum requirements, leaving maintainers without a
secure, repeatable approach for selecting tools, publishing images, and configuring a project.

## What Changes

- Add a published guide for a project-specific, credential-free sandbox image and link it from the
  project setup documentation.
- Describe the default split between the project image and the runtime and harness image volumes,
  so projects do not add the runtime, Node.js, or an adapter unnecessarily.
- Recommend a multi-stage build that retains only the repository's required tools and runtime
  dependencies, normally omitting the package manager from the final image.
- Explain when a maintained minimal distribution, distroless image, or `FROM scratch` image is
  appropriate, including the compatibility requirement for `git` and a shell.
- Document pull-request validation, trusted main-branch publication, credential exclusion, and
  immutable digest selection for a project's configured image.

## Capabilities

### New Capabilities

None. This change documents the existing project-image and harness-delivery behavior without
changing it.

### Modified Capabilities

None. This change does not alter a system requirement.

## Impact

- Published documentation in `docs/content/using/`, plus a link from the existing project setup
  page.
- No changes to application behavior, configuration, APIs, container images, dependencies, or
  credentials.
