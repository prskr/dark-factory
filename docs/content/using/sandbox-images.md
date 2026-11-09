---
title: Build a sandbox image
linkTitle: Sandbox images
weight: 2
---

Each project gets its own **sandbox image**: build one image for the repository that agents work
on. It is the agent's operating environment, not a general-purpose developer workstation. Include
only the tools that repository's agents need to inspect, build, test, and validate its changes.

## What Dark Factory supplies

The project image must contain:

- `git` and a shell;
- the repository's required toolchain and validation tools;
- runtime libraries, CA certificates, and data required by those tools.

Dark Factory mounts the agent runtime at `/opt/factory` and, by default, the selected harness at
`/opt/harness/<harness>` as read-only image volumes. Do not add the runtime, Node.js, a coding
CLI, or an ACP adapter just to run the default harness. Add an adapter only when the project sets
an **Adapter command**, which replaces the injected harness.

Run the image as a non-root user. Keep credentials, SSH keys, registry logins, cloud configuration,
and source-control tokens out of it. Dark Factory provides the credentials it manages at the point
of use; an image must never become a way to distribute secrets.

## Start from the repository's work

List the commands agents must run in this repository—for example, a formatter, unit tests, a
compiler, a deployment manifest validator, or a browser test client. Add exactly their executables
and required libraries to the image. Do not copy a broad local development environment into every
project image "just in case".

When a repository needs another tool, change its image definition, review it, rebuild the image,
and update the project's configured digest. Do not expect an agent to install the tool during a
run: the sandbox's network access is restricted, and mutable runtime installation makes its
environment harder to review and reproduce.

## Build a small final image

Use a multi-stage build where practical:

1. In a **builder** stage, use the package manager and build tools needed to install, compile, or
   collect the repository's required programs.
2. In the **final** stage, copy only the programs, their runtime dependencies, certificate bundle,
   `git`, and a shell. Set a non-root `USER` and the workspace as the working directory.
3. Do not leave a package manager in the final image unless agents have an explicit, reviewed need
   to use it at runtime.

Leaving out unused packages, build tools, package metadata, and the package manager reduces the
image's executable attack surface and the number of dependencies that need security updates. It
also keeps agent environments reproducible: a run cannot silently change its toolchain by
installing a newer package. This is a hardening preference, not an excuse to omit a required tool
or its security updates.

## Choose the final base

Use the smallest **maintained** base that can safely carry the final contents. Choose the most
restrictive option that you can build and validate for the repository:

| Final base | Use it when | Check carefully |
| --- | --- | --- |
| Maintained minimal distribution | It is the simplest way to supply the required tools and their shared libraries. | Remove build-only packages and package metadata; rebuild for base-image security updates. |
| Distroless image | You can deliberately supply `git`, a shell, CA certificates, all needed shared libraries, and the project tools. | Distroless images intentionally omit many common debugging and installation utilities. Test every required command in the final image. |
| `FROM scratch` | The required programs are static/self-contained, or you deliberately copy their loader, shared libraries, certificates, `git`, and shell. | Nothing is supplied implicitly: missing files, users, time-zone data, a shell, or dynamic-library dependencies cause failures. |

Distroless and `scratch` images are valuable because they remove unnecessary operating-system
programs and libraries, reducing attack surface and patching scope. They are not universal
requirements. Dark Factory still requires `git` and a shell in every project image, so select a
maintained minimal distribution when that is the safer, more supportable choice.

## Validate, publish, and select the image

Treat the image definition as repository code:

1. Build the image and run the repository's required commands in it on every pull request. Confirm
   it has no credentials and that its non-root user can clone, build, test, and use `git`.
2. Publish only from a reviewed, trusted branch. Publish an immutable identifier for each build,
   such as a commit-derived tag, and retain enough build metadata to trace it to source.
3. Resolve the published image to its content digest and configure that digest as the project's
   **Sandbox image**. Do not configure a mutable tag such as `latest`: it can change an approved
   sandbox environment without a project configuration change.
4. Rebuild and validate after changes to the image definition, its base image, or included tools,
   including security updates. Review the new digest before selecting it for the project.

The registry, builder, CI system, and supported architectures are project decisions. The security
properties are the same: a reviewed image contains only the required tools and no credentials, and
the project selects an immutable image identity.
