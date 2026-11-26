## MODIFIED Requirements

### Requirement: Public key on the persona page
The group "Commits" on the Commits tab of the persona settings page (requirement "Persona settings page" of `configuration-management`, approved PenPot boards) SHALL show the commit email, the public signing key in OpenSSH format and its SHA-256 fingerprint, each with a copy action, a link to the how-to for registering them on the forge's bot account, and the button "Rotate key". Without a key it SHALL say so instead of showing the key.

#### Scenario: Registering on the forge
- **WHEN** a board member opens the Commits tab of persona `linus`
- **THEN** the group "Commits" shows, in this order: the editable "Commit email" `linus@example.org` with its existing hint; "Register the public key and the commit email on the forge's bot account, so that the forge shows this persona's commits as verified."; the link "Verify persona commits on the forge"; the read-only "Public key" `ssh-ed25519 AAAA…`; the read-only "Fingerprint" `SHA256:…`; the button "Rotate key"

#### Scenario: Copy actions
- **WHEN** a board member opens the Commits tab of persona `linus` in a secure context
- **THEN** the copy buttons "Copy commit email", "Copy public key" and "Copy fingerprint" copy the full values and show "Copied", like the webhook URL; without a secure clipboard they are hidden

#### Scenario: No key yet
- **WHEN** a board member opens "New persona", or the Commits tab of persona `ada` that has no key
- **THEN** the group "Commits" shows the commit email field and "No signing key yet. Save the persona to create it.", and no "Rotate key" button
