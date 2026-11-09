# Tasks

## 1. Publish the sandbox-image guide

- [x] 1.1 Add `docs/content/using/sandbox-images.md` with the project-image boundary, minimum requirements (`git` and a shell), credential-free and least-tooling guidance, a multi-stage build pattern, package-manager omission guidance, and the maintained-minimal-base, distroless, and `FROM scratch` compatibility trade-offs; verify the guide makes no project-specific infrastructure or credential assumptions.
- [x] 1.2 Document provider-neutral image delivery: pull-request build validation, trusted-branch publication, rebuilds for included-tool and base-image updates, and immutable-digest project configuration rather than a mutable tag; verify the advice does not claim support for a particular registry or CI provider.

## 2. Integrate and verify the published documentation

- [x] 2.1 Update `docs/content/using/setup.md` to link the Sandbox image field to `sandbox-images.md`; verify the setup page remains a concise configuration reference.
- [x] 2.2 Run `just docs-build` and verify the published site builds with no broken links.
