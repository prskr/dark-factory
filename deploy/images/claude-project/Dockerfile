# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
# Reference Claude Code project image for Dark Factory sandboxes (openspec change harness-images, design D8).
# Debian Trixie (13) with git, bash, curl, CA certificates and the OpenSSH client, UID 1000 and /workspace.
# It contains no Node.js and no adapter: the control plane mounts the harness image at /opt/harness/claude-code.

FROM debian:trixie-slim@sha256:f6b08b09a05725f9431502d3b5714f5c4576e82c1a84fa1debe0c24a7e0c3ec4

RUN apt-get update && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        openssh-client \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 1000 --create-home --shell /bin/bash dev \
    && mkdir -p /workspace \
    && chown 1000:1000 /workspace

USER 1000:1000
WORKDIR /workspace
