# ADR-0008: Configuration split between provisioning and protocol

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

Runtime configuration (environment, config files, secrets) can be injected when the sandbox is provisioned or delivered over the agent protocol. The question was where this logic should live.

## Decision

- **Provisioning (Sandbox / pod spec):** identity and static configuration — project image, runtime image volume, runtime class, resources, csi-driver certificate volume, task PVC, control plane endpoint.
- **Protocol:** everything per run — prompts and queued messages, harness configuration, skills, tool list, short-lived credentials.
- Exception: the Claude subscription token must reach the harness environment; it's delivered per run over the stream onto tmpfs, never in the pod spec.

## Consequences

- Pod specs contain no task-specific data or secrets.
- Persona and tool changes take effect on the next run without touching Kubernetes objects.

## Alternatives considered

- **Everything in the pod spec**: secrets in pod specs, and every config change means new Kubernetes objects.
