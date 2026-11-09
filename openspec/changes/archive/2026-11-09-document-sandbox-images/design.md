# Design

## Context

See [proposal.md](proposal.md) for the motivation. The published project setup page currently states
only that an image needs `git` and a shell. Existing behavior mounts the runtime and, by default,
the selected harness as read-only image volumes, while each project chooses its own image.

## Goals / Non-Goals

**Goals:**

- Add one concise, scenario-based guide at `docs/content/using/sandbox-images.md`.
- Give maintainers a secure image-content and delivery baseline that applies to any project.
- Link the guide from the Sandbox image field explanation in `docs/content/using/setup.md`.

**Non-Goals:**

- Change image provisioning, harness delivery, image-pull authentication, or the project form;
  rejected.
- Require a particular distribution, container builder, registry, CI provider, or image
  architecture; rejected.
- Provide a runnable image definition for every project type; rejected.
- Document private homelab registries, image names, infrastructure, or credentials; rejected.

## Decisions

### Make the guide the canonical project-image how-to

The new guide owns the image-build and lifecycle instructions. The setup page stays a short
configuration reference and links to it from the Sandbox image row. This avoids turning the setup
page into an implementation guide while making the advice discoverable where the image is set.

### Describe a security-first final image without making one base mandatory

The guide recommends a per-project, credential-free final image containing only the tools an agent
needs. It explains a multi-stage build: install and compile in a builder, then copy only the needed
programs, libraries, certificates, and data into the runtime image. A package manager should not
be present unless an agent needs it at runtime.

The guide presents a maintained minimal distribution as the compatible baseline, distroless as the
next option when `git`, a shell, and all runtime dependencies are deliberately supplied, and
`FROM scratch` only for fully self-contained/static programs or deliberately copied dependencies.
It states that `git` and a shell remain required by Dark Factory even for distroless or scratch
images. Making distroless or scratch mandatory was rejected because toolchain compatibility and
patchability take precedence over a nominally smaller image.

### Use generic image-delivery guidance

The guide will show a provider-neutral flow: validate builds in pull requests, publish only from a
reviewed trusted branch, use immutable content-addressed image digests in project configuration,
and rebuild promptly when the base image or included tools need security updates. It will prohibit
embedding credentials and state that mutable tags such as `latest` are unsuitable for a configured
project image. Registry-specific CI examples were rejected to keep the guide reusable.

### Clarify the default image boundary

The guide will distinguish the project image from the runtime and injected harness image volumes.
It will tell maintainers not to add the runtime, Node.js, a coding CLI, or an ACP adapter merely to
support the default harness; an adapter belongs in the project image only when the project chooses
the explicit adapter-command override.

## Risks / Trade-offs

- [A minimal image omits a required dynamic library, certificate bundle, executable, or shell] →
  State the compatibility requirements and require project-specific build/run validation before
  publication.
- [Removing the package manager makes interactive dependency installation impossible] → Preserve
  it only when that is an intentional runtime requirement; otherwise rebuild the image after
  changing declared tools.
- [A mutable tag changes a sandbox environment after approval] → Direct projects to use a resolved
  immutable digest and retain the source tag only as build metadata.
- [A guide becomes stale as image tooling evolves] → Describe stable security outcomes and Dark
  Factory's existing image boundary instead of tying the guide to one builder or registry.
