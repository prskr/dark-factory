# Design boards: persona-commit-signing

- Status: approved (2026-11-25; Forgejo key verification boards added 2026-11-25; Forgejo domain field added 2026-11-25)
- Tokens added: none
- Components added or changed: none

## Boards

| Board (page / name) | Route | Data | Viewport |
|---|---|---|---|
| Config - Personas Signing / Personas · edit modal, commits | `/personas/<id>/edit` | persona `linus` (`linus@example.org`) with a signing key, instance without the `forgejo` plugin (no "Verify key on Forgejo"). Partial board: modal only | 1440 |
| Config - Personas Signing / Personas · commits, no key yet | `/personas/<id>/edit` | persona without a key (or `/personas/new`). Partial board: group "Commits" only | 1440 |
| Config - Personas Signing / Personas · commits, key rotated | `/personas/<id>/edit` | as Personas · edit modal, commits, after "Rotate key" was confirmed. Partial board: group "Commits" only | 1440 |
| Config - Personas Signing / Personas · rotate key confirmation | `/personas/<id>/edit` | `linus` without active runs; "Rotate key" clicked. Partial board: dialog only | 1440 |
| Config - Personas Signing / Personas · rotate key confirmation, active runs | `/personas/<id>/edit` | `linus` with 2 active runs; "Rotate key" clicked. Partial board: dialog only | 1440 |
| Config - Personas Signing / Personas · rotate key failed | `/personas/<id>/edit` | Secret write fails on confirm. Partial board: dialog only | 1440 |
| Config - Personas Signing / Phone · edit persona sheet, commits | `/personas/<id>/edit` | as Personas · edit modal, commits; sheet scrolled to "Commits" | 390 |
| Config - Personas Signing / Personas · commits, Forgejo | `/personas/<id>/edit` | as Personas · edit modal, commits, on an instance with the `forgejo` plugin. Partial board: group "Commits" only | 1440 |
| Config - Personas Signing / Personas · verify key on Forgejo | `/personas/<id>/edit` | `linus`, "Verify key on Forgejo" clicked, token empty, "Forgejo domain" prefilled `forgejo.example.org`. Partial board: dialog only | 1440 |
| Config - Personas Signing / Personas · verify key on Forgejo, signed | `/personas/<id>/edit` | as above after "Sign token" with a valid token. Partial board: dialog only | 1440 |
| Config - Personas Signing / Personas · verify key on Forgejo, failed | `/personas/<id>/edit` | token over 256 characters submitted. Partial board: dialog only | 1440 |

## Decisions the specs don't carry

- Groups use `form_group` (15 px semibold heading, top border between groups), the same component as the project form. "Commit email" moves from the identity row into "Commits"; Identity keeps avatar, Display name, Handle (one row) and Description.
- Order in "Commits": Commit email (editable input + copy button, existing hint below), register hint (`text-sm text-muted`), how-to link (`text-accent-text underline`), Public key, Fingerprint, the "Key rotated" status line (only after a rotation), "Rotate key".
- Public key and Fingerprint use `READONLY_INPUT_CLASS` (mono, `bg-sunk`, muted text) and truncate with an ellipsis on one line; the copy action copies the full value.
- Copy buttons: `copy_button` pattern of `crates/df-web/src/project_webhook.rs` (icon-only `ICON_BUTTON_CLASS`, Lucide `copy`, check icon while copied, hidden without a secure clipboard). Labels: "Copy commit email", "Copy public key", "Copy fingerprint".
- "Rotate key" is a secondary button below the fields, left-aligned; it is not part of the form's save and opens the confirmation. No rotate button without a key.
- "Key rotated…" status: check icon plus `text-sm font-medium text-accent-text`, like the webhook "Copied" status.
- Rotate confirmation reuses the dialog of the reset-workspace confirmation (`Tasks - Status / Desktop · reset workspace confirmation`): title, two paragraphs, buttons "Cancel" (secondary) and "Rotate key" (danger). With active runs only the second paragraph changes. The error line (`text-danger`, `role="alert"`) sits between the paragraphs and the buttons; the dialog stays open.
- Phone: same group layout in the sheet; inputs and icon buttons 44 px, "Rotate key" 48 px high. The rotate confirmation uses the existing phone dialog style (no separate board).
- The copy buttons are drawn locally on the boards; the `Icon button` component has no `Copy` variant.
- "Verify key on Forgejo" (only with the `forgejo` plugin and a key): secondary button in one row with "Rotate key", left of it, gap 8 px. It is not part of the form's save and opens the verify dialog. Phone: the two buttons stack full width, 48 px high, "Verify key on Forgejo" first.
- Verify dialog: same dialog as the rotate confirmation (title, paragraphs, buttons right-aligned), but "Sign token" is the primary button (`color.accent`, like "Save persona"), "Cancel" secondary. The "Token" field is a normal text input without a copy button; placeholder in `text-faint`.
- "Forgejo domain" field: below "Token", same input style; prefilled with the first `forgejo` route's host (`apiUrl`, else claimed host) as a value (`text-text`, not a placeholder), editable. Hint below in the style of the commit-email hint (12 px, `text-faint`). The invalid-domain state has no own board: it is the failed state's error line with the domain reason.
- Signed state: replaces the paragraphs and the token field in the same dialog (no second dialog). "Signature" is a multi-line read-only block (`READONLY_INPUT_CLASS`, mono, `whitespace-pre`, no truncation, all lines visible) with the copy button "Copy signature" (same `copy_button` pattern) aligned to its top. Only one button: "Done" (primary), which closes the dialog.
- Failed state: the error line (`text-danger`, `role="alert"`) between the "Forgejo domain" field and the buttons, as in the rotate dialog; the entered token and domain stay in their fields.
- Phone: the verify dialog uses the existing phone dialog style (no separate board); the signature block scrolls horizontally if a line does not fit.
