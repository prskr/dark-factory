---
title: Security
weight: 9
---

Dark Factory runs code that an AI agent writes and executes. The design assumes that the agent, the
repository contents and everything the agent produces are untrusted, and puts the protection at the
boundary of the sandbox instead of inside it.

## What is trusted

| Trusted | Untrusted |
| --- | --- |
| The control plane, the gateway, the `migrate` job | The harness and every process it starts |
| PostgreSQL, the Kubernetes API, cert-manager | The repository contents, including `.git`, hooks and configuration files |
| The humans with the `board-member` role | Everything on the task volume at the start of a run |
| The forge, the model provider (as services, not their content) | Tool output, web content and anything an agent sends to the control plane |

## Principles

- **The sandbox is the blast radius.** Inside its pod the agent may do anything. Protection covers
  what crosses the boundary: network traffic, Git pushes, messages to the control plane.
- **No long-lived credentials in the sandbox.** The forge credential never enters it: the
  [Git proxy](git-proxy.md) adds it on the way out, and the control plane calls the forge API itself.
  The only secret a sandbox holds is the harness credential (for example the Claude subscription
  token), delivered over the agent channel into memory, never through the pod spec or a file.
- **Git is the only durable store for code.** The task volume is a cache; the runtime regenerates
  the files it owns (`.git/config`, hooks path) before using the repository.
- **Default deny on the network.** A sandbox reaches the control plane, the gateway and nothing else
  unless a rule allows it. See [Egress gateway](egress-gateway.md).
- **Every privileged change is audited.** Egress rules, push policies, stops, cancellations,
  decisions on requests and every push attempt are recorded with the actor.

## Sandbox isolation

| Layer | Setting |
| --- | --- |
| Runtime class | Kata Containers microVM where configured (`sandboxNamespace.runtimeClassName`); without it the sandbox shares the node kernel |
| Pod security | The sandbox namespace enforces Pod Security `restricted`: non-root, no privilege escalation, all capabilities dropped |
| Kubernetes API | No service account token (`automountServiceAccountToken: false`); the sandbox namespace holds no Secrets |
| Network | A generated NetworkPolicy per run: egress only to the control plane agent port, the gateway and allowed cluster services; no ingress; no DNS |
| Images | The runtime and the harness are mounted as read-only image volumes; no container daemon, no nested containers |

## Identity

Each sandbox pod gets its own short-lived certificate from cert-manager's CSI driver, with the URI
SAN `spiffe://dark-factory/ns/<namespace>/pod/<name>/<uid>`. The private key never lands in a
Kubernetes Secret. The control plane accepts a connection only if the namespace is the sandbox
namespace, the pod exists with that UID and the instance's labels, and the connection comes from the
pod's IP, so a copied key is useless from anywhere else. The gateway identifies sandboxes by source
pod IP through a registry the control plane pushes to it.

Every action of an agent is bound to its run: sandbox identity, run, task, persona and project
decide which repository, branches, hosts and tools it may use.

## Humans

Users log in through OIDC (see [Authentication](../authentication/)); every page needs the role
`board-member`. Sessions are server-side; the browser only holds an HTTP-only cookie. Agents cannot
approve their own requests: approvals need a human, and the control plane applies the approved
action itself.

## Known limits

- Without Kata Containers the sandbox is a regular container on the node kernel.
- The egress proxy sees host names and TLS handshakes, not request contents (no TLS interception).
  An allowed host is allowed with every path.
- Network traffic that ignores the proxy settings is dropped by the NetworkPolicy and does not show
  in the live traffic view.
- Dark Factory is pre-MVP and has had no external security review.
