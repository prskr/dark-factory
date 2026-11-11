# Review log: ui-model-provider-page

- BASE: a90a1853815d9ff026892322f5b7ca5e99b08a01
- Store: none
- Status: stopped (artifact update pending)
- Implementer agent: (not started)
- DOCKER_HOST: unix:///var/run/docker.sock
- Step: 1 (design), task 1.1 in progress

## Design session (2026-11-11)

Task 1.1, page "Config - Model providers", boards built (1440 px):

- "Model providers · settings page, general": project settings frame (back link, name heading, General / Models page tabs, "Save model provider"); General form as in the edit modal, without Preset
- "Model providers · settings page, models, saved": "Model provider saved" status; Models tab with one line per model across the panel width (Model ID fills, Wire API 220 px, Context and Max output 140 px, remove), full list without inner scroll. Layout approved by the user.
- "Model providers · settings page, model errors": the errors of the old edit board on the page layout

Not done yet: the old "Model providers · edit, …" boards are still on the page (to delete once the page boards are approved); tasks 1.2 to 1.5.

User decision: drop the phone case entirely. The page has no entry in the phone navigation, so the 390 px boards were removed. This changes the `model-providers` delta (scenario "Page on a phone", "full page below 768 px" in "Model provider settings page"), design D1/D4 and task 1.1 ("at 1440 and 390 px"), and task 7.2 (390 px check). Run `/opsx:update` with this, commit, then restart `/implement-change ui-model-provider-page`; it resumes at step 1.
