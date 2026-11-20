-- The structured event of a queued webhook message (`transcript-readability` design D2): an
-- encoded `darkfactory.transcript.v1.WebhookReceived`. NULL for every other message. The
-- control plane turns it into a forge activity row when the message is delivered.
ALTER TABLE task_messages ADD COLUMN forge_event BYTEA;
