-- Deny rules (iter2-egress-deny-rules design D1): `http-deny` entries name one host, apply to
-- every port (no port) and exist at instance scope only.
ALTER TABLE egress_allowlist DROP CONSTRAINT egress_allowlist_kind_check;
ALTER TABLE egress_allowlist ADD CONSTRAINT egress_allowlist_kind_check CHECK (
    (kind = 'http-host' AND host IS NOT NULL AND port IS NOT NULL)
    OR (kind = 'cluster-service' AND namespace IS NOT NULL AND service IS NOT NULL)
    OR (kind = 'http-deny' AND host IS NOT NULL AND port IS NULL AND scope = 'instance')
);
