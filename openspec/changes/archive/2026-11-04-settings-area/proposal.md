# Proposal

## Why

The desktop rail has eight icon-only sections and grows with every configuration page. Pages for daily work (Tasks, Requests, Projects, Network traffic) and setup pages that are touched rarely (Personas, Credentials, Model providers, Egress rules) compete for the same icons, which gets harder to scan with each new one. A navigation rail works best with three to seven destinations.

## What Changes

- **BREAKING** The rail drops Personas, Credentials, Model providers and Egress rules. It keeps the logo, the new-task action, Tasks, Requests, Projects and Network traffic, and gets a Settings link (gear) at the bottom above the avatar.
- Personas, Credentials, Model providers and Egress rules become sections of a settings area: a settings sidebar between the rail and the page lists them with icon and label and marks the open one. Settings is marked current in the rail on each of them.
- The pages keep their URLs (`/personas`, `/credentials`, `/model-providers`, `/egress/allowlist`); the Settings link opens `/personas`.
- Phones are unchanged: the settings area is desktop only, and personas and credentials stay out of the phone navigation (`configuration-management`, "Configuration pages on phones").

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-ui-shell`: "Desktop navigation rail" lists the new sections and the Settings link.
- `configuration-management`: new requirement for the settings area and its sidebar.

## Impact

- `crates/df-web`: `components/shell.rs` (`SECTIONS`, rail), `layout.rs` (`Nav`, settings sidebar for the settings sections), the pages of personas, credentials, model providers and egress rules (layout only), tests.
- Other planned changes: `iter3-chats` modifies the same rail requirement.
- The Model providers page (archived change `iter3-inference-gateway`) exists as a rail entry today and moves into the settings sidebar.
- Docs: pages that tell users to open Personas, Credentials, Model providers or Egress rules from the rail.
- Design: PenPot "App Design", page "Config - Personas", board "Settings · personas". The shared `Rail` component (page "Components") is updated when this change is built.

Out of scope:
- Settings on phones: rejected for now (phone navigation keeps out configuration pages on purpose).
- Moving the pages to `/settings/...` URLs: rejected (no user gain, breaks links from the traffic page and the docs).
- Instance and user settings pages (roadmap iteration 5, "Settings entry in the profile menu"): they join this settings area when they are built.
