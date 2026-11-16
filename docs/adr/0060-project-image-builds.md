# ADR-0060: Project image builds

- **Status:** Accepted
- **Date:** 2026-11-16
- **Related:** [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md), [ADR-0030](0030-kubernetes-only-with-sandbox-provider-seam.md), [ADR-0039](0039-repository-configuration-activated-by-approval.md), [ADR-0059](0059-repository-file-reads-through-the-forge-plugin.md)

## Context

Each project names its own sandbox image (ADR-0022); building and publishing it is left to the project's team. Dark Factory can build the image itself from the repository's `.dark-factory/` directory, which agents can never change (always-protected path). A build runs `RUN` steps from that directory, needs the internet to fetch packages and base images, and needs credentials to push to a registry. Sandboxes on the other hand have no direct internet and reach hosts only through the gateway's per-run allowlist (ADR-0029).

## Decision

- **Builds are trusted workloads in their own namespace.** A build namespace, created or referenced by the chart like the sandbox namespace, holds the build pods and the operator's push Secret. A static NetworkPolicy allows DNS, the internet and operator-listed cluster targets. Builds don't use the gateway.
- **Builder and pusher are separate containers.** The builder (an init container, default a maintained Kaniko fork) gets the build context and writes an image tarball to an `emptyDir`; it has no credentials. The pusher (`crane`) then pushes the tarball with the push Secret and reports the digest. Builder and pusher images and arguments are operator settings; the interface is the context directory in and the tarball out.
- **Sandboxes reference built images by digest.** The tag (`<context hash>-<build number>`) is for humans; a run pins the digest of the project's latest successful build when it is provisioned.
- **Failed builds block runs.** While a project's latest build that wasn't cancelled hasn't succeeded, its runs don't start. There is no fallback to an earlier image.
- **Builds start only on a human's request** and use the current commit of the base branch, without the `project-config` approval of ADR-0039.

## Consequences

- Build steps can reach any internet host. This is accepted because only humans write `.dark-factory/`, and the builder holds no secret it could leak.
- The build namespace is a second namespace with control plane write access (pods, pod logs, ConfigMaps); the operator manages the push Secret there. The control plane never reads it.
- A project whose build fails is stopped until someone fixes the build context or retries. Unpinned dependencies can break a rebuild of an unchanged context.
- Builds without a registry-backed cache start cold.
- Kaniko runs as root, so the build namespace uses Pod Security level `baseline`, not `restricted`.

## Alternatives considered

- **Building in the sandbox namespace, through the gateway:** rejected. The gateway allows only registered sandbox pod IPs and per-run allowlists, and the sandbox namespace must contain no Secrets.
- **One container that builds and pushes:** rejected. Every `RUN` step would run next to the push credentials.
- **BuildKit or Buildah as default builder:** rejected for now. Rootless BuildKit needs an unconfined seccomp and AppArmor profile (Pod Security `privileged`); either can replace Kaniko later behind the same interface.
- **Falling back to the previous image after a failed build:** rejected. Runs would silently use an image that no longer matches the repository.
- **Tags instead of digests in sandboxes:** rejected. A tag can be overwritten; a digest names exactly the image that was built.
