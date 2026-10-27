# Review log: transcript-view-polish

- BASE: 7a35b5e9de2b7386f7a1b4d690deab43711ff9c8
- Store: none
- Status: done
- Implementer agent: aef612cbcf7323a8b

- Design commit: cf52ddd

## Round 1 (2026-10-27)

- Implementer: 9911dd1, tasks 7/7, checks fmt/clippy/machete ok, df-web 330 and df-db 45 pass, container tests ran (podman socket)
- Reviewed range: 81e9249...9911dd1

Implementer browser check (task 4.2, Playwright container, 1440x900, seeded data removed): transcript opens at the bottom (distance 0); after scrolling up a live event leaves `scrollTop` unchanged; at the bottom a new tall item keeps the view at the bottom; opening another task lands at the bottom; 7 queued messages: header "7 queued · Linus reads them at the next turn", three 36 px rows, list scrolls (276 px in 116 px), input stays at the viewport bottom; runs start closed, open state and chevron change, a live new run updates the count to 4 and the list stays open. Open question from the implementer: `AbortError` from `hx-sse` at `htmx_before_cleanup` when swapping tasks (not traced; likely same cause as 001e81e).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S4+P3 | Standards+Spec | P1 | `docs/design-system.md:180` | "Queued stack" bullet still describes a card; header, one-line row, three-row cap missing | fix-now | fixed (3dca9bd) |
| S3 | Standards | P1 | `view.rs:328` | `pending_region` doc comment misses `persona`, header, cap | fix-now | fixed (3dca9bd) |
| S1 | Standards | P1 | `view.rs:322,350` | "failed or completed" check duplicated | fix-now | fixed (3dca9bd) |
| P1 | Spec | P1 | `task_pages/tests.rs:375` | scroll scenarios only checked as attributes; 48 px threshold not pinned (browser check recorded above) | fix-now | fixed (3dca9bd) |
| P2 | Spec | P1 | `layout.rs:40` | no test pins `open` in `morphIgnore`, runs scenario depends on it | fix-now | fixed (3dca9bd) |
| I1 | Impl | P2 | `hx-sse` swap | `AbortError` on task swap, untraced | tech-debt | tech-debt (TD-62) |
| S2 | Standards | P3 | `view.rs:333` | Data Clumps: key/persona/status/pending | cosmetic | cosmetic |
| S5 | Standards | P3 | `view.rs:505` | summary classes duplicate section header classes | cosmetic | cosmetic |
| S6 | Standards | P3 | `task_pages.rs:243` | long JS one-liner in Rust string | cosmetic | cosmetic |
| P4 | Spec | P3 | `view.rs:890` | 7-queued layout only covered by browser check (recorded) | cosmetic | cosmetic |

User decision (round 1): Fix blocking + P1 (S4+P3, S3, S1, P1, P2).

## Round 2 (2026-10-27)

- Implementer: 3dca9bd, tasks 7/7, fmt/clippy ok, df-web 331 pass, container tests ran
- Reviewed range: 9911dd1...3dca9bd

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S1 | Standards | P1 | `layout.rs:112-127` | new test sits between two `use` lines, duplicate `use super::HTMX_CONFIG` | fix-now | fixed (fix round 2) |
| R2-S3 | Standards | P1 | `docs/design-system.md:180` | header and cap belong to `pending_region`, not `queued_message` | fix-now | fixed (fix round 2) |
| R2-S2 | Standards | P3 | `view.rs:317` | private `task_ended` between public fns (copies `queued_state` placement) | cosmetic | cosmetic |

Spec axis: no findings (11/11 scenarios).

User decision (round 2): Fix blocking + P1 (R2-S1, R2-S3).

Fix round 2 (commit wip fix round 2) (R2-S1, R2-S3), df-web 331 pass, clippy/fmt ok, container tests ran. Diff checked by the orchestrator, no further findings; no third reviewer pass.
Tech debt created: TD-62.
- Design commit (task 6.1): see next commit `wip(transcript-view-polish): design many runs`

## Round 3 (2026-10-27): amendment "runs scroll area" (tasks 6.1 to 6.4)

- Implementer: iteration 2 commit, tasks 11/11, fmt/clippy/machete ok, df-web 333 pass, container tests ran; df-db not run (no df-db change)
- Reviewed range: fix round 2 commit to iteration 2 commit (the implementation and design commits; artifact commits included for the spec)
- Implementer browser check: 1440x900 body does not scroll, list scrolls, description 3 lines, live insert keeps the list open and the offset; collapsing leaves only the header; 1280x720 with a tall upper block the body scrolls and the 17 rem minimum holds; phone sheet scrolls as a whole. Note: scroll scrollTop +78 px on a live insert at the top through scroll anchoring (the visible card stays in place, the list stays open) (scroll anchoring); collapsed `#runs` still fills the remaining height (no border).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-S1 | Standards | P1 | `input.css:185-212` | `.ws-details` flex/overflow declared in both media blocks and the base rule overrides `overflow-y-auto`; move into the base rule | fix-now | fixed (4c07f60) |
| R3-S4+P2 | Both | P1 | `task_pages/tests.rs:1673` | test only checks selector strings; assert `min-height:17rem`, `flex:1 1 0%`, `md:shrink-0` on `#task-details` | fix-now | fixed (4c07f60) |
| R3-S5 | Standards | P1 | `task_pages/tests.rs:1673` | `#[tokio::test]` without `.await`; use `#[test]` | fix-now | fixed (4c07f60) |
| R3-S3 | Standards | P1 | `input.css:213-229` | 17rem comment on the wrong rule; no note on the `::details-content` fallback | fix-now | fixed (4c07f60) |
| R3-S7 | Standards | P1 | `docs/design-workflow.md:122` | `--demo-flows` does not seed tasks or runs; write the real data source (manual seed, not built) for the new row and the two earlier rows | fix-now | fixed (4c07f60) |
| R3-P3 | Spec | P1 | browser check | 768 to 1279 px slide-over never checked; check "Many runs" at 1024 px and record it | fix-now | fixed (4c07f60) |
| R3-P1 | Spec | P1 (artifact, not blocking) | `specs/task-ui/spec.md` | requirement lacks the exception (upper block taller than the column: details scroll as a whole) and the two-card minimum; needs `/opsx:update`, not an implementer fix | user decision | fixed (spec fixup) |
| R3-S2 | Standards | P3 | `input.css:215-226` | `.ws-runs` and `::details-content` rules have the same body | cosmetic | cosmetic |
| R3-S6 | Standards | P3 | `view.rs`, `overlay.rs`, `input.css` | scroll layout split between utilities and custom CSS | cosmetic | cosmetic |
| R3-N1 | Spec | rejected | live insert offset +78 px | scroll anchoring, spec only requires the list to stay open | rejected (not a violation, recorded in the browser note) | rejected |
| R3-N2 | Spec | rejected | collapsed `#runs` fills height | invisible, no border; only the header shows | rejected (not a violation) | rejected |

User decision (round 3): Fix blocking + P1. R3-P1 (spec wording) is left to /opsx:update by the user.

Fix round 3 (4c07f60): R3-S1, R3-S4+P2, R3-S5, R3-S3, R3-S7, R3-P3 fixed; fmt/clippy/machete ok, df-web 333 pass, container tests ran. 1024 px slide-over check: the aside and body do not scroll, the run list scrolls (616 px in 315 px), sections above moved by 0, collapse leaves only "Runs 8". Diff checked by the orchestrator, no further findings; no fourth reviewer pass.
R3-P1 fixed by the user via a spec update (exception and two-card minimum, scenario "Little room").
- Design commit (task 7.1, tool rows and run cards): see the next commit `wip(transcript-view-polish): design tool rows and run cards`

## Round 4 (2026-10-27): amendment tool rows, turn end, run link, token table (tasks 7.1 to 7.8)

- Implementer: iteration 3 commit, tasks 18/18, fmt/clippy/machete ok, df-web 338 pass, container tests ran; df-db not run (no df-db change)
- Reviewed range: pre-amendment squash commit `feat(df-web): transcript follow...` to HEAD (design and implementation of the amendment)
- Implementer browser check: consecutive tool rows share one line (also after a live append), chevron up when open, light and dark, no "Turn ended" text, long model name ellipsized in the 320 px column, run link jumps with follow mode off (also after a live event), phone sheet closes on the link click.
- Note from the implementer: the Lucide wrench path was typed from memory (no source at hand); `compact_count` cuts instead of rounding (999999 gives 999k).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R4-S1+P1 | Both | Blocking | `sse.rs:171-178` | the `TurnEnded` branch is now the only live refresh of `#runs` and `#task-details` after a turn (usage comes from the `turnEnded` JSON); no test fails if it is removed (verified: sse tests only check `#runs` in the first frame) | fix-now | fixed (fix round 4) |
| R4-S2 | Standards | P1 | `sse.rs:175` | full path `df_proto::transcript::v1::event::Kind::TurnEnded` inline; import it | fix-now | fixed (fix round 4) |
| R4-S3 | Standards | P1 | `task_pages/tests.rs:1676,1692` | same `rule` closure in two tests; one helper | fix-now | fixed (fix round 4) |
| R4-S5 | Standards | P1 | `crates/df-web/README.md:47` | one 330-character line between wrapped paragraphs | fix-now | fixed (fix round 4) |
| R4-P3 | Spec | P1 | `components/transcript.rs:163` | wrench uses `text-faint`, design says muted | fix-now | fixed (fix round 4) |
| R4-P2 | Spec | P1 | browser check | long model name in the phone sheet and tool rows at phone width not recorded | fix-now | fixed (fix round 4) |
| R4-S4 | Standards | P3 | `icons.rs:101` | `Wrench` arm first, enum order after `Logo` | cosmetic | cosmetic |
| R4-W | Standards | note | `icons.rs` | wrench path is the newer Lucide revision (rounded jaw), differs from the older path; offline, not verified against lucide.dev | user may compare | fixed (verified against lucide.dev) |

User decision (round 4): Fix blocking + P1 (R4-S1+P1, R4-S2, R4-S3, R4-S5, R4-P3, R4-P2).

Fix round 4: all six items fixed; fmt/clippy/machete ok, df-web 339 pass, container tests ran. The new SSE test fails when the `TurnEnded` branch is disabled (mutation check). Phone check at 390 px: tool rows single line without overflow, run card with the long model name in the sheet without overflow, `title` carries the full name. Diff checked by the orchestrator, no further findings; no fifth reviewer pass.
Open for the user: compare the wrench path in `components/icons.rs` with lucide.dev (R4-W). No new tech debt this round.

R4-W resolved: the wrench path in `icons.rs` is identical to the lucide.dev SVG (checked by the user).
