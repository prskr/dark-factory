---
title: Dark Factory documentation
linkTitle: Documentation
# Every page of the docs build uses Hextra's docs layout (sidebar, breadcrumbs).
cascade:
  type: docs
---

Dark Factory is a Kubernetes-native control plane for AI coding agents. It is pre-MVP: releases may
change configuration, the database schema and the web UI incompatibly. Pick the docs of the release
you run in the version menu.

<!-- Card links are relative (no leading `/`): Hextra passes `/…` through relURL, which drops the /docs/<slot>/ prefix. -->
{{< cards >}}
  {{< card link="getting-started/" title="Getting started" icon="play" subtitle="Install a release with Helm into a cluster with Kata Containers, an external PostgreSQL and an OIDC provider." >}}
  {{< card link="using/" title="Using Dark Factory" icon="cursor-click" subtitle="Projects, personas, tasks, requests and network access in the web UI." >}}
  {{< card link="authentication/" title="Authentication" icon="lock-closed" subtitle="Log in through Keycloak or Dex; the role `board-member` and the control plane settings." >}}
  {{< card link="database/" title="Database" icon="database" subtitle="The PostgreSQL owner and application roles, and how to provision them." >}}
  {{< card link="integrations/" title="Integrations" icon="puzzle" subtitle="Forgejo and GitHub through forge plugins." >}}
  {{< card link="observability/" title="Observability" icon="chart-bar" subtitle="Log format and levels, OpenTelemetry metrics and traces." >}}
  {{< card link="operations/" title="Operations" icon="cog" subtitle="Upgrades, backup and restore, troubleshooting." >}}
  {{< card link="architecture/" title="Architecture" icon="cube" subtitle="Components, connections and the life of an agent run." >}}
  {{< card link="security/" title="Security" icon="shield-check" subtitle="The security model, the Git proxy and the egress gateway." >}}
  {{< card link="reference/" title="Reference" icon="book-open" subtitle="Configuration of the sandbox runtime and the agent channel." >}}
{{< /cards >}}
