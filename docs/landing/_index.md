---
title: Dark Factory
layout: hextra-home
---

{{< hextra/hero-badge link="#pre-mvp" >}}
  <div class="hx:w-2 hx:h-2 hx:rounded-full hx:bg-primary-400"></div>
  Pre-MVP: expect incompatible changes
{{< /hextra/hero-badge >}}

<div class="hx:mt-6 hx:mb-6">
{{< hextra/hero-headline >}}
  AI coding agents&nbsp;<br class="hx:sm:block hx:hidden" />on your own cluster
{{< /hextra/hero-headline >}}
</div>

<div class="hx:mb-12">
{{< hextra/hero-subtitle >}}
  A Kubernetes-native control plane for agents&nbsp;<br class="hx:sm:block hx:hidden" />that work on your software projects
{{< /hextra/hero-subtitle >}}
</div>

<div class="hx:mb-6">
{{< hextra/hero-button text="Get started" link="/docs/" style="background: #00ff41; color: #0c0a09;" >}}
</div>

<div class="hx:mt-6 hx:mb-16 hx:w-full">
  <img src="images/network-traffic.webp" width="1440" height="900" loading="eager"
    alt="The network traffic view: three sandboxes, the hosts they contacted, and the allowed and blocked connections"
    style="width: 100%; height: auto; border-radius: 1.5rem; border: 1px solid var(--df-line); box-shadow: 0 0 120px -20px rgba(0, 255, 72, 0.35);" />
</div>

<div id="features" class="hx:mb-6 hx:w-full" style="scroll-margin-top: 5rem;">
{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Tasks and chats"
    subtitle="Describe a task, pick a persona and follow the live transcript. Send follow-up messages and answer questions from your phone."
    style="background: radial-gradient(ellipse at 50% 80%, rgba(0, 255, 72, 0.12), hsla(0, 0%, 100%, 0));"
  >}}
  {{< hextra/feature-card
    title="Sandboxed runs"
    subtitle="Every run gets its own Kubernetes pod, in a Kata Containers microVM where available. A small Rust runtime runs Claude Code or another ACP harness."
    style="background: radial-gradient(ellipse at 50% 80%, rgba(56, 189, 248, 0.12), hsla(0, 0%, 100%, 0));"
  >}}
  {{< hextra/feature-card
    title="Controlled network access"
    subtitle="Sandboxes reach only a gateway with host allowlists. A live view shows every connection, and a blocked host is one click from a rule."
    style="background: radial-gradient(ellipse at 50% 80%, rgba(239, 68, 68, 0.12), hsla(0, 0%, 100%, 0));"
  >}}
  {{< hextra/feature-card
    title="Git is the only state"
    subtitle="Agents clone, commit and push in their own workspace. Dark Factory opens the draft pull request; review stays in your forge."
  >}}
  {{< hextra/feature-card
    title="Forges as plugins"
    subtitle="Forgejo and GitHub integrations are sandboxed WebAssembly components. A new forge does not change the control plane."
  >}}
  {{< hextra/feature-card
    title="Lean by design"
    subtitle="A compiled control plane and runtime, one PostgreSQL database per instance, no shared tenancy. Cheap enough to run several instances."
  >}}
{{< /hextra/feature-grid >}}
</div>

<div class="content hx:w-full hx:mt-10">

## Pre-MVP

Dark Factory has no stable release line and no production installation yet. Versions are named
`YYYY.M.N`, and every release may change configuration, the database schema or the web UI
incompatibly. Features that are designed but not built are marked as planned in the
[README](https://github.com/prskr/dark-factory#what-it-does).

## Who it is for

- You run Kubernetes yourself, with Kata Containers, cert-manager and a CNI that enforces network policies.
- You have your own Git forge (Forgejo or GitHub) and an OIDC provider such as Keycloak.
- You want agents to use a Claude subscription instead of API pricing.
- You are fine with an early project and willing to read the docs and the source.

## Who it is not for

- **No Kubernetes**: Dark Factory runs only on Kubernetes. There are no SSH or cloud runtimes and no plan for them.
- **No Kata Containers**: without a microVM runtime, sandboxes share the node kernel. That works, but the isolation is weaker than the design intends.
- **Multi-tenant needs**: one instance and one database belong to one tenant. There is no SaaS mode and none is planned.
- **Stability needs**: until the first stable version, expect incompatible changes between releases and no migration guarantees.
- **Code review in a UI**: review happens in your forge on the pull request, not in Dark Factory.

</div>
