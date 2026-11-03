# Review log: brand-host-icons

- BASE: 1a514a1b3a98367321cb0602d43ef83c24815a8c
- Store: none
- Status: done
- Implementer agent: adf501e886090642f

## Round 1 (2026-11-03)

- Implementer: 578281a, tasks 13/14 (3.3 open: no browser login to seeded instance), checks fmt/clippy/machete/file-length/test-names/openspec validate ok, df-web 468 + df-core 92 tests pass, container tests ran (podman)
- 1.4: contrast from tone tokens ≥ 6.3:1 light, ≥ 8.3:1 dark; screenshots of static host_icon markup, not the live rules page
- Reviewed range: 1a514a1...578281a
- Security: 0 findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P3 | Spec | Blocking | `crates/df-web/src/components/host_icon.rs:200-230` | "Known providers" scenario: npm, Go, Google brand names/glyphs untested | fix-now | fixed (c5fff23) |
| P1+P2 | Spec | Blocking | tasks 1.4, 3.3 | No live browser check (rules + traffic page, both themes, seeded GitHub project); 1.4 ticked on static markup | live check by orchestrator | fixed (live check 2026-11-03) |
| S1 | Standards | P1 | `crates/df-web/src/components/icons.rs:185,216-234` | `brand_body` re-matches variants with `_ => ""` catch-all; new brand silently empty | fix-now: per-glyph consts like `LOGO_BODY` | fixed (c5fff23) |
| S5+P7 | Standards+Spec | P1 | `docs/design-system.md:105` | Doc claims `project_roles` is the one role rule (API host comes from `from_projects`); missing scenario "Project host before provider monogram" | fix-now | fixed (c5fff23) |
| P4 | Spec | P1 | `crates/df-web/src/egress/tests/icons.rs` | "Forge API on a separate host" only unit-tested, no page-level icon test | fix-now | fixed (c5fff23) |
| P5 | Spec | P1 | `crates/df-core/src/routing.rs:739` | "unrouted URL" test uses unparsable `"not a url"` | fix-now | fixed (c5fff23) |
| P6 | Spec | P1 | `crates/df-web/src/egress/tests/rules.rs:381` | No rules-page test for `api.github.com` without project | fix-now | fixed (c5fff23) |
| S2 | Standards | P3 | `crates/df-web/src/components/host_icon.rs:32-87` | githubusercontent/pythonhosted entries duplicate provider fields | — | cosmetic |
| S6 | Standards | P3 | `crates/df-web/src/components/project_host.rs:88-92` | Role mapping split between `project_roles` and `from_projects` (as designed in D3) | — | cosmetic |
| S8 | Standards | P3 | `crates/df-web/src/testkit.rs:243` | `HostWithApiUrl` fully qualified instead of imported | — | cosmetic |
| S3 | Standards | Rejected | `host_icon.rs:28` | Monogram fallback speculative | — | rejected (design D2 keeps it on purpose; spec scenario "Provider without a glyph") |
| S4 | Standards | Rejected | `host_icon.rs:126` | Fallback `data-host-icon` should be `brand-<name>` | — | rejected (design D1: fallback stays the monogram) |
| S7 | Standards | Rejected | `docs/design-system.md:105` | Scenario names in doc prose | — | rejected (task 2.4 requires naming them; conflicts with P7) |

User decision: Fix blocking + P1 (P3, S1, S5+P7, P4, P5, P6). P1+P2 live browser check: user runs `just serve-local` + `just seed` and reports; 1.4/3.3 stay open until then.

## Round 2 (2026-11-03)

- Implementer: c5fff23, tasks 12/14 (1.4 unticked, 3.3 open: user runs live browser check), checks fmt/clippy/machete/file-length/test-names ok, df-web 470 + df-core 92 pass, container tests ran (podman)
- Reviewed range: 578281a...c5fff23
- Security: 0 findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S9+P8 | Standards+Spec | Blocking | `crates/df-web/src/egress/tests/rules.rs:384-391` | `github.com` / `api.github.com` icon assertions `rfind` over the page prefix, so each can pass on the other row's icon; P6 not really covered (code changed in fix round 1) | fix-now: scope both to `row_of` | fixed (e2bcfcc) |
| S10 | Standards | P1 | `crates/df-web/src/egress/tests/rules.rs:361` | Provider-glyph-without-project check folded into an unrelated test name | fix-now: own test named after behavior | fixed (e2bcfcc) |
| S11+S12 | Standards | P1 | `crates/df-web/src/components/icons.rs:61,73-91` | Licence doc repeated 5×; enum doc points to `Icon::body` for licence, now on consts | fix-now: one licence note above the consts, point enum doc there | fixed (e2bcfcc) |

User decision: Fix blocking + P1 (S9+P8, S10, S11+S12).

## Round 3 (2026-11-03)

- Implementer: e2bcfcc, checks fmt/clippy/machete/file-length/test-names ok, df-web 471 pass, container tests ran (podman); mutation check: both row assertions fail with a wrong expected icon
- Reviewed range: c5fff23...e2bcfcc
- Security: 0 findings. Spec: 0 findings; S9+P8, S10, S11+S12 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S13 | Standards | P3 | `crates/df-web/src/egress/tests/rules.rs:396-400` | `row_of(&page, ">ghcr.io<")` called twice although `ghcr` is bound | — | cosmetic |

### Live browser check (orchestrator, Playwright, 2026-11-03)

- `just serve-local` + `just seed`, project `GH` (repo `https://github.com/org/df.git`, credential `forgejo-bot`) created via UI; project list shows the GitHub forge icon.
- Rules page: `github.com` and `api.github.com` → `forge-github`; `raw.githubusercontent.com` → `brand-github`; anthropic, npm, go, python (pypi + pythonhosted), google glyphs; `ghcr.io` globe. Computed contrast light 6.37–9.42, dark 8.32–10.62 (≥ 3:1).
- Traffic page (demo flows): `github.com` → `forge-github` in table and graph; provider glyphs correct; IP → server. Demo flows never hit `api.github.com`, so that host was not seen live on the traffic page (covered by page test `a_project_host_and_its_forge_api_host_beat_the_provider_glyph`).
- Observation: Go glyph (wide "GO" wordmark) is very small at table size.

User decision: approve live check, tick 1.4 and 3.3 (api.github.com on the traffic page covered by page test). Go glyph accepted as is. Brand colours: rejected (monochrome passes ≥ 3:1), no roadmap row.

## Finish

- Status: done. No blocking finding open; container-backed tests ran in every round (podman).
- Bazel lockfile build: skipped, no `Cargo.toml` / `Cargo.lock` change.
- Tech debt: none (no P2 findings). Roadmap: no rows (out-of-scope items and brand colours rejected).
- Tasks 12/12, 3 review rounds, 2 fix rounds.

## Round 4 (2026-11-03): scope extension

- User request: more providers (google.com, gcr.io, pkg.dev → Google; bazel.build → Bazel; k8s.io, kubernetes.io → Kubernetes; ghcr.io → GitHub; go.dev, goproxy.io → Go; nodejs.org → Node.js; rust-lang.org, crates.io → Rust; gitlab.com → GitLab). Folded into this change via /opsx:update (proposal, design D2, both spec deltas, tasks section 4).
- Implementer: 3e8886c + 438e237 (clippy doc fix), tasks 4.1–4.3, 4.5; checks fmt/clippy/machete/file-length/test-names/openspec validate ok, df-web 471 + df-core 92 pass, container tests ran (podman). All five new glyphs from simple-icons@14.0.0.
- Reviewed range: 01b8ce4...438e237 (includes the artifact commit)
- Security: 0 findings.
- Live check 4.4 (orchestrator, Playwright): all 21 provider domains + example.org (globe) correct on the rules page; contrast light 6.37–9.42, dark 7.89–10.62. Kubernetes and Node.js marks small but recognisable at table size.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S14 (was S2) | Standards | P1 | `crates/df-web/src/components/host_icon.rs:32-180` | 21 `BRANDS` entries for 11 providers; 10 copy name/monogram/tone/glyph | fix-now: `domains: &[&str]`, one entry per provider | fixed (12d8c44) |
| S15 | Standards | P1 | `crates/df-web/src/components/icons.rs:325-337` | Filled-glyph test keeps its own brand list | fix-now: iterate `BRANDS` glyphs | fixed (12d8c44) |
| P10 | Spec | P1 | `crates/df-web/src/components/host_icon.rs:333-336` | Provider test checks only `brand-<name>`, not that the glyph matches | fix-now: assert the expected `Icon` body too | fixed (12d8c44) |
| P9 | Spec | P1 | `docs/development.md:158` | "`ghcr.io` the issue icon instead of the globe": now GitHub glyph | fix-now: wording | fixed (12d8c44) |
| P11 | Spec | P1 | `crates/df-web/src/components/project_host.rs` | No test for a GitLab project's repo host keeping `forge-git` over `brand-gitlab` (design D2) | fix-now | fixed (12d8c44) |
| S16 | Standards | P3 | `docs/design-system.md:105` | Full 21-domain list repeated in prose | — | cosmetic |
| S17 | Standards | P3 | `specs/egress-log/spec.md:9-10` | Two parallel 21-item lists in one scenario | — | cosmetic |

User decision: Fix all P1 (S14, S15, P10, P9, P11).

| D2-note | Spec | P3 | `openspec/changes/brand-host-icons/design.md` D2 | "a GitLab project's repository host keeps the generic Git icon" is too absolute: on an instance with a default (host-less) plugin, that plugin serves gitlab.com and the icon shows its forge (existing routing from iter2-multi-forge) | — | cosmetic |

## Round 5 (2026-11-03)

- Implementer: 12d8c44, checks fmt/clippy/machete/file-length/test-names/openspec validate ok, df-web 472 pass, container tests ran (podman)
- Reviewed range: 438e237...12d8c44
- Security: 0 findings. Spec: 0 findings; P9, P10, P11 confirmed fixed, S14 refactor keeps look-alike, subdomain, wildcard, unknown-host and monogram behaviour. Standards: S14, S15 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S18 | Standards | P3 | `crates/df-web/src/components/host_icon.rs:243-265` | 21-row test table repeats provider name/glyph (kept as the spec oracle) | — | cosmetic |
| S19 | Standards | P3 | `crates/df-web/src/components/host_icon.rs:243,268` | Test binding `name` holds `brand-github`, not `Brand::name` | — | cosmetic |
| S20 | Standards | P3 | `crates/df-web/src/components/host_icon.rs:290-291` | `projects` vs `no_credential` naming | — | cosmetic |

## Finish (round 5)

- Status: done. No blocking finding open; container-backed tests ran in every round (podman). Live check 4.4 done by the orchestrator.
- Bazel lockfile build: skipped, no `Cargo.toml` / `Cargo.lock` change.
- Tech debt: none (no P2). Roadmap: no rows.
- Tasks 17/17, 5 review rounds, 4 fix rounds (incl. the scope extension).

## Round 6 (2026-11-03): helm.sh

- User request: `get.helm.sh` gets a Helm icon. Artifacts updated directly (proposal, design D2, egress-log "Known providers" now 22 domains, task 4.6).
- Implementer: 9a4533d, Helm glyph from simple-icons@14.0.0, tone `waiting`; checks fmt/clippy/machete/file-length/test-names/openspec validate ok, df-web 472 pass, container tests ran (podman)
- Reviewed range: 18f9695...9a4533d
- Security: 0 findings. Spec: no code findings. Live check 4.6: `get.helm.sh` → `brand-helm`, contrast light 6.37, dark 10.39; mark small at table size, like Kubernetes, but recognisable.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S21+P13 | Standards+Spec | P1 | `openspec/changes/brand-host-icons/review-log.md` | Reopening rewrote the status lines of the earlier Finish sections | fixed by orchestrator: only the header status changes | fixed |
| P12 | Spec | P3 | `openspec/changes/brand-host-icons/tasks.md:26` | Task 4.1 says "21 domains" (state before helm.sh; 4.6 extends it) | — | cosmetic |

## Finish (round 6)

- Status: done. No blocking finding open; container-backed tests ran (podman); live check 4.6 done by the orchestrator.
- Bazel lockfile build: skipped, no `Cargo.toml` / `Cargo.lock` change.
- Tech debt: none. Roadmap: no rows.
- Tasks 18/18, 6 review rounds, 5 implementation/fix rounds after the first.

## Round 7 (2026-11-03): AWS

- User request: AWS logo for `amazonaws.com` (S3 host `gitlabcommunity-eu1-eu-west-1-prod-packages.s3.eu-west-1.amazonaws.com`) and other AWS domains. User chose `amazonaws.com`, `ecr.aws`, `awsstatic.com`, `aws.amazon.com`, `cloudfront.net`. Glyph: Simple Icons `amazonwebservices` (present in 14.0.0). Artifacts updated directly (proposal, design D2, egress-log "Known providers" now 27 domains, task 4.7).
- Implementer: 8701481, AWS glyph `amazonwebservices` from simple-icons@14.0.0 (single path), tone `queued`; checks fmt/clippy/machete/file-length/test-names/openspec validate ok, df-web 473 pass, container tests ran (podman)
- Reviewed range: 02e5656...8701481
- Security: 0 findings. Live check 4.7: S3 host, `public.ecr.aws`, `d111111abcdef8.cloudfront.net`, `console.aws.amazon.com` → `brand-aws`, contrast light 9.42, dark 10.18; `www.amazon.com` → globe. AWS wordmark small at table size but readable.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S23+P14 | Standards+Spec | P1 | `docs/design-system.md:105` | "and" before GitLab instead of AWS; CloudFront operator note from D2 missing | fixed by orchestrator (doc only) | fixed |
| S22 | Standards | P3 | `crates/df-web/src/components/host_icon.rs:89-101`, `icons.rs:104-105` | AWS entry/const order differs from enum and lists | — | cosmetic |

## Finish (round 7)

- Status: done. No blocking finding open; container-backed tests ran (podman); live check 4.7 done by the orchestrator.
- Bazel lockfile build: skipped, no `Cargo.toml` / `Cargo.lock` change.
- Tech debt: none. Roadmap: no rows.
- Tasks 19/19.

## Round 8 (2026-11-03): project host icon size

- User report: `github.com` (forge icon, project `GH`) and `ghcr.io` (provider glyph) render at different sizes.
- Cause (verified): the project-role tile in `host_icon` sizes `[&>svg]:size-4` (direct child only), but `role_icon` / `forge_icon` wrap the svg in a `span`, so project-host icons keep `icon()`'s default `size-5` (20 px) while provider glyphs are 16 px. Present since BASE; visible now that provider glyphs sit next to forge icons.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| U1 | User | Blocking | `crates/df-web/src/components/host_icon.rs:143` | Project-host icons (forge, Forgejo, Git, issue) are 20 px instead of 16 px in the host tile | fix-now with regression test | fixed (cc51d1e) |
- Implementer: cc51d1e, checks fmt/clippy/machete/file-length/test-names ok, df-web 474 pass, container tests ran (podman); class already in checked-in app.css, no CSS rebuild needed
- Reviewed range: 66ac969...cc51d1e
- Security: 0 findings. Live check: every host glyph is 16x16 on the rules page (24 px tile) and traffic page (24 px table, 28 px graph), forge icons included.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P15 | Spec | P1 | `crates/df-web/src/components/host_icon.rs:317-336` | Size regression test misses forge-git role and server tile | fix-now | fixed (09f3f76) |
| P17 | Spec | P1 | `docs/design-system.md:105` | Host icon section states no tile or glyph size | fix-now | fixed (09f3f76) |
| P16 | Spec | P3 | `crates/df-web/src/components/host_icon.rs:197-205` | Test renders only `HostIconSize::Table`; class is size-independent today | — | cosmetic |
| S24 | Standards | P3 | `crates/df-web/src/components/host_icon.rs:143,151,163` | Glyph tile class prefix written 3× | — | cosmetic |

User decision: Fix both P1 (P15, P17).

### Fix round 8b

- Implementer: 09f3f76 (test adds forge-git and server tiles; design-system.md states tile 24/28/36 px and glyph 16 px). Checks fmt/clippy/file-length/test-names ok, df-web 474 pass, container tests ran.
- Review: diff is test + one doc sentence; checked by the orchestrator against the code (`size-6`/`size-7`/`size-9` frames), no reviewer agents spawned.

## Finish (round 8)

- Status: done. No blocking finding open; U1 fixed and confirmed live (all glyphs 16 px); container-backed tests ran (podman).
- Bazel lockfile build: skipped, no `Cargo.toml` / `Cargo.lock` change.
- Tech debt: none. Roadmap: no rows.
- Tasks 19/19.

## Round 9 (2026-11-03): Codeberg

- User request: Codeberg logo for `codeberg.org`. Simple Icons `codeberg` exists in 14.0.0. Artifacts updated directly (proposal, design D2 incl. Forgejo project-role note, egress-log "Known providers" now 28 domains, task 4.8).
- Implementer: a152de3 + c7e6818 (clippy + doc fix), Codeberg glyph from simple-icons@14.0.0, tone `running`; `#[expect(clippy::too_many_lines)]` added on `Icon::body` (101 lines); checks fmt/clippy/machete/file-length/test-names/openspec validate ok, df-web 475 pass, container tests ran (podman)
- Reviewed range: 581bda7...c7e6818
- Security: 0 findings. Spec: 0 findings. Live check 4.8: `codeberg.org` → `brand-codeberg`, 16x16, contrast light 6.59, dark 8.32.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S25 | Standards | Blocking | `crates/df-web/src/components/icons.rs:177-180` | `#[expect(clippy::too_many_lines)]` on `Icon::body` although a split exists (GitHub and Git still inline) | fix-now: `GITHUB_BODY`, `GIT_BODY`, drop the expect | fixed (afce416) |
| S26 | Standards | — | `review-log.md` | Follow-up of the expect not recorded as tech debt | moot once S25 is fixed | moot (afce416) |
| S27 | Standards | P3 | `crates/df-web/src/components/host_icon.rs:356` | Codeberg test repeats the `brand-codeberg` assertion of the provider table test | — | cosmetic |

User decision: Fix S25.

### Fix round 9

- Implementer: afce416 (`GITHUB_BODY`, `GIT_BODY` consts; `#[expect(clippy::too_many_lines)]` removed; `body` 99 physical lines). Checks fmt/clippy/file-length/test-names ok, df-web 475 pass, container tests ran.
- Review: diff is a behaviour-free move of two string literals into consts; checked by the orchestrator, no reviewer agents spawned.

## Finish (round 9)

- Status: done. No blocking finding open; container-backed tests ran (podman); live check 4.8 done by the orchestrator.
- Bazel lockfile build: skipped, no `Cargo.toml` / `Cargo.lock` change.
- Tech debt: none. Roadmap: no rows.
- Tasks 20/20.

## Round 10 (2026-11-03): github.io

- User request: `github.io` (GitHub Pages) gets the GitHub glyph. Added to the GitHub entry; artifacts updated directly (design D2, egress-log "Known providers" now 29 domains, task 4.9).
- Implementer: 0004e4f (`github.io` added to the GitHub entry, test rows for `github.io` and `prskr.github.io`, doc list). Checks fmt/clippy/file-length/test-names/openspec validate ok, df-web 475 pass, container tests ran.
- Review: diff is one domain string, two test rows and a doc list entry; checked by the orchestrator, no reviewer agents spawned.
- Live check 4.9: `prskr.github.io` → `brand-github`, 16x16 (tone and contrast as the other GitHub hosts).

## Finish (round 10)

- Status: done. No blocking finding open; container-backed tests ran (podman).
- Bazel lockfile build: skipped, no `Cargo.toml` / `Cargo.lock` change.
- Tech debt: none. Roadmap: no rows.
- Tasks 21/21.

## Round 11 (2026-11-03): Linux distributions

- User request: Debian logo for `debian.org`, plus Ubuntu, Alpine, Fedora and Red Hat. Domains: `debian.org`, `ubuntu.com`, `alpinelinux.org`, `fedoraproject.org`, `redhat.com`, `redhat.io`. All five glyphs exist in simple-icons@14.0.0 as single paths. Artifacts updated directly (proposal, design D2, egress-log "Known providers" now 35 domains, task 4.10).
- Implementer: b63c779 (five distro glyphs, Lucide bodies moved to consts, `body` 88 lines). Not reviewed separately: superseded by the storage change below; the distro tests carry over.
- User decision: move the provider table to an embedded `brands.json` (design D1/D2 rewritten, task 4.11). Redirect requested, but b63c779 had already landed; the storage change follows as its own round.
- Implementer: 4e6af22 (provider table as `brands.json`, 19 providers; brand variants removed from `Icon`; `compile_data` in BUILD.bazel). Checks fmt/clippy/machete/file-length/test-names/openspec validate ok, df-web 476 pass, Bazel build and test ok, container tests ran.
- Reviewed range: f5e976d...4e6af22 (distros b63c779 + storage change)
- Security: 0 findings. Live check 4.10/4.11 (data-file build): all 36 rule rows show the expected icon at 16 px; distros light 6.38–7.57, dark 7.89–10.62; GitHub forge and provider icons render from the table.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P21 | Spec | Blocking (regression of P10) | `crates/df-web/src/components/host_icon.rs:155-193` | Provider test reads the expected path from the table it checks; swapped paths pass | fix-now: pinned path prefix per provider | fixed (214599a) |
| S33+P25 | Standards+Spec | Blocking (regression of S23) | `docs/design-system.md:105` | Comma before "AWS" missing again | fix-now | fixed (214599a) |
| S29+P22 | Standards+Spec | P1 | `crates/df-web/src/components/icons.rs:5,210` | `brand_glyph("github")` falls back to empty markup; Octocat path not pinned | fix-now | fixed (214599a) |
| S30+P19 | Standards+Spec | P1 | `crates/df-web/src/components/host_icon.rs` (guard test) | Guard test parses its own struct without `tone`; unknown fields not rejected | fix-now: check `BRANDS`, `deny_unknown_fields` | fixed (214599a) |
| P20 | Spec | P1 | `crates/df-web/src/components/host_icon.rs:228-235` | Path charset allows every letter | fix-now: SVG command letters + `e`/`E` | fixed (214599a) |
| S28 | Standards | P1 | `crates/df-web/src/components/host_icon.rs:46-63` | Private items between public ones | fix-now | fixed (214599a) |
| S31 | Standards | P1 | `crates/df-web/src/components/icons.rs:70-92` | Lucide consts no longer needed (`body` 68 lines) | fix-now: inline again | fixed (214599a) |
| S32 | Standards | P1 | `crates/df-web/BUILD.bazel:77` | Test `compile_data` repeats `brands.json` | fix-now, confirm with Bazel test | fixed (214599a) |
| S34 | Standards | P1 | `crates/df-web/src/components/host_icon.rs:39-43` | Comment repeats the `expect` message | fix-now | fixed (214599a) |
| P23 | Spec | P1 | `openspec/changes/brand-host-icons/tasks.md` 4.11 | "markup stays the same" vs `<path></path>` | fixed by orchestrator (task text) | fixed |
| P24 | Spec | P1 | `openspec/changes/brand-host-icons/tasks.md` 4.10 | Task describes `Icon` variants that 4.11 removes | fixed by orchestrator (task text) | fixed |

User decision: Fix blocking + P1.
- Implementer: 214599a (fix round 11), checks fmt/clippy/machete/file-length/test-names/openspec validate ok, df-web 476 pass, Bazel test ok, container tests ran; P21 swap experiment fails as expected
- Reviewed range: ac93493...214599a
- Security: not re-run (round 11 found nothing; round 12 diff is behaviour-free apart from the visible Git fallback). Standards and spec: all round 11 fixes confirmed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P26 | Spec | P1 | `openspec/changes/brand-host-icons/design.md` D1, D2 | D2 lacked the Git-icon fallback of `brand_glyph`; D1 guard list lacked `deny_unknown_fields` and the strict charset | fixed by orchestrator (design text) | fixed |
| S35 | Standards | P3 | `crates/df-web/src/components/icons.rs:68,169` | `GIT_BODY` const could be inline again like the other Lucide bodies | — | cosmetic |
| S36 | Standards | P3 | `crates/df-web/src/components/host_icon.rs:279-280` | Guard test re-checks the github path already pinned elsewhere | — | cosmetic |
| P27 | Spec | P3 | `crates/df-web/src/components/host_icon.rs:194-246` | A provider added later without a test row has no pinned prefix (trade-off with "one JSON object, no Rust change") | — | cosmetic |

## Finish (round 12)

- Status: done. No blocking finding open; container-backed tests ran (podman); Bazel df-web build and test ran (BUILD.bazel changed); live check 4.10/4.11 done by the orchestrator on the data-file build (fix round 11 changed no glyph data; provider tests pin path prefixes).
- Tech debt: none. Roadmap: no rows.
- Tasks 23/23.
