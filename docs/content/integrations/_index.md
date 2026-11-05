---
title: Integrations
weight: 5
---

Dark Factory talks to external systems through plugins: WebAssembly components that run inside the
control plane in a sandbox (Wasmtime). A plugin sees no credentials; the control plane adds them to
each request and limits the plugin to the hosts of its route.

- [Forges](forges/): Forgejo and GitHub, for pull requests, issues and review feedback;
  [forge webhooks](forges/webhooks.md) wake tasks on reviews, mentions, merges and CI failures.

Outgoing, without a plugin:

- [Lifecycle events](lifecycle-events.md): signed webhooks when a task volume or a run sandbox is
  created or destroyed.
