---
title: Lifecycle events
weight: 2
---

Dark Factory notifies external systems when a task's volume or a run's sandbox is created or destroyed. Each event is an HTTP POST with a JSON body, signed with the instance's Ed25519 key in the [Standard Webhooks](https://www.standardwebhooks.com/) `v1a` scheme. Delivery is at least once, retried for 24 hours, and never delays a run.

Typical receivers: wake a GPU or build node before a sandbox needs it, track sandbox minutes per project, or feed an audit log.

## Subscriptions

| | Instance subscription | Project subscription |
|---|---|---|
| Receives | events of every project | events of one project |
| Managed in | control plane configuration (Helm `lifecycleEvents.subscriptions`), restart to apply | UI: Projects → edit a project → tab "Lifecycle events"; needs the role `admin`, other users do not see the tab |
| URL | `http://` or `https://`, any address including cluster-internal services | `https://` only; the host must resolve to public addresses ([Address filter](#address-filter)); no username or password in the URL (it is written to the audit log) |
| Delivery status | control plane logs | last attempt (time, HTTP status or error) in the tab "Lifecycle events" |

Each subscription lists the event types it receives. An empty list means all types, including types added later.

The instance public key is served without authentication at `GET /lifecycle-events/public-key` (`text/plain`, `whpk_<base64>`) and shown at the top of every project's tab "Lifecycle events". Without a signing key the feature is off: nothing is recorded, the endpoint returns 404 and the tab says so (existing subscriptions stay listed).

Instance subscriptions in the Helm values (reference: [chart README](https://github.com/prskr/dark-factory/blob/main/deploy/helm/dark-factory/README.md#values-reference), `lifecycleEvents.*`):

```yaml
lifecycleEvents:
  subscriptions:
    - url: http://audit.observability.svc:8080/events
      events: [sandbox.destroyed]
```

Outside the chart, the control plane `config.yaml` takes the same list plus the key path:

```yaml
lifecycleEvents:
  signingKeyFile: /run/secrets/lifecycle-events/tls.key # Ed25519, PEM PKCS#8
  subscriptions: [] # as above
```

With subscriptions configured but no `signingKeyFile`, the control plane refuses to start.

## How-to: add a project subscription and verify its events

1. Get the role `admin` ([Authentication](../authentication/_index.md); Keycloak: [client role](../authentication/keycloak.md), Dex: [group](../authentication/dex.md)).
2. Open **Projects**, click the edit icon of the project and choose the tab **Lifecycle events**, then **New subscription**. The form opens inside the tab.
3. Enter the receiver URL (`https://`, public host), check the event types (none: all) and click **Create subscription** or press Enter in the URL field. An invalid URL keeps the form open with an error at the field.
4. Copy the public key `whpk_…` from the tab into the receiver's configuration.
5. In the receiver, verify every delivery ([Verifying signatures](#verifying-signatures)) and drop duplicates by `webhook-id`.
6. Start a task in the project. Its first run sends `sandbox.provisioning`, `volume.provisioned` and `sandbox.provisioned`. The tab then shows the last attempt, for example `HTTP 204`.

Subscriptions save on their own: creating, editing and deleting take effect at once, "Save project" does not apply to them, and cancelling the project dialog keeps them. The pencil icon of a row edits it in place. One form is open at a time: opening another discards the unsaved values of the first. The trash icon asks "Delete this subscription? Deliveries to it stop at once." in the row; only **Delete** removes the subscription.

A failing receiver shows the error or status of its last attempt in the tab (`address refused: …`, `timed out`, `connection failed: …`, `HTTP 503`). Delivery is retried in the background ([Delivery](#delivery)).

## How-to: wake a node before a sandbox starts

Goal: a node that is normally off (a GPU box on the LAN, a spot VM) boots when a run needs it and shuts down when no run uses it.

1. Use an **instance subscription**. A project subscription refuses private LAN addresses, so a wake-on-LAN bridge in the cluster or on the LAN must be an instance receiver:

   ```yaml
   lifecycleEvents:
     subscriptions:
       - url: http://wol-bridge.infra.svc:8080/events
         events: [sandbox.provisioning, sandbox.provisioning_failed, sandbox.destroyed]
   ```

2. **Wake on `sandbox.provisioning`.** The scheduler emits it when it picks the run, before it creates the volume and the sandbox; a run waiting in the queue emits nothing yet. Dark Factory does not wait for the receiver: the sandbox pod stays `Pending` until the node joins and can take it. The node must be ready within `runs.connectTimeoutSeconds` (default 600 s), which counts from run creation. Otherwise the run ends with `connect_timeout`.
3. **Shut down by matching per run.** Every `sandbox.provisioning` is followed by exactly one `sandbox.provisioning_failed` or `sandbox.destroyed` with the same `data.run.id`. Keep the set of open run IDs: add on `sandbox.provisioning`, remove on either closing event. Delivery order is not guaranteed, so remember closed run IDs for a while and ignore a late `sandbox.provisioning` for one of them. Shut the node down after the set has been empty for a grace period.
4. Make each step idempotent: the same event can arrive twice (same `webhook-id`).

To target only some projects, filter on `data.project.key` in the receiver.

## Event catalog

Every event has the same envelope:

| Field | Content |
|---|---|
| `id` | `evt_` + UUIDv7; equals the `webhook-id` header |
| `type` | event type, see below |
| `timestamp` | RFC 3339 time of the state change (not of the delivery attempt) |
| `data.project` | `id`, `key`, `repo_url` |
| `data.task` | `id`, `key` |
| `data.persona` | `id`, `name` |
| `data.run` | `id`, `number`; missing for `volume.destroyed` |

Values are a snapshot at the time of the event: a later rename does not change events already recorded.

| Type | When | Extra fields in `data` |
|---|---|---|
| `sandbox.provisioning` | The scheduler starts provisioning a run, before volume and sandbox are created. Once per run. | none |
| `volume.provisioned` | The task volume (`task-<key>`) was created for the first time. Once per task; again after the volume was deleted. Carries the run that created it. | none |
| `sandbox.provisioned` | The provider reports the run's pod as running. Does not wait for the agent to connect. | none |
| `sandbox.provisioning_failed` | Creating the volume or the sandbox failed. No `sandbox.destroyed` follows for the run. | `error` (text) |
| `sandbox.destroyed` | A run whose provisioning began ended and its teardown was issued. | `outcome`, `started_at` (when the pod came up, `null` if it never did), `ended_at` |
| `volume.destroyed` | The task volume was deleted. | `reason`: `retention` or `reset` |

`outcome` is one of `completed`, `end_turn`, `blocked`, `limit_reached`, `rate_limited`, `cancelled`, `failed`, `connect_timeout`, `sandbox_lost`. A run cancelled while still provisioning gets `cancelled` with `started_at: null`.

Where each event fires in a run: the sequence diagram in [design/agent-run.md §6.0](https://github.com/prskr/dark-factory/blob/main/design/agent-run.md#60-sequence-as-implemented).

Example delivery (`sandbox.provisioned`, captured from the e2e suite; the body is sent compact, formatted here):

```http
POST /hook HTTP/1.1
content-type: application/json
webhook-id: evt_01a1a35b-6980-74e1-a69a-5ff710ca948d
webhook-timestamp: 1793742039
webhook-signature: v1a,qx0/bYj8Yk3QuiiRYr4LnjrJI9RdOU6TMWz/5ZS52HZj8MBsL2mAeOUIuCcz4kRAmg6ixJxF4lICOBwqj5SFBA==
```

```json
{
  "id": "evt_01a1a35b-6980-74e1-a69a-5ff710ca948d",
  "data": {
    "run": {"id": "01a1a35b-5603-70f9-a97d-40ff01958ead", "number": 1},
    "task": {"id": "01a1a35b-55f9-7464-b9de-d50e9dab11f8", "key": "DF-1"},
    "persona": {"id": "01a1a35b-23f3-75a5-8fe4-4a97a86da9bf", "name": "e2e"},
    "project": {
      "id": "01a1a35b-23fc-7048-b801-f2b4602bc70b",
      "key": "DF",
      "repo_url": "http://forgejo-http.forgejo.svc:3000/factory/demo"
    }
  },
  "type": "sandbox.provisioned",
  "timestamp": "2026-11-03T21:40:39.422Z"
}
```

## Delivery

Request: `POST <url>` with the envelope as body and these headers:

| Header | Value |
|---|---|
| `content-type` | `application/json` |
| `webhook-id` | event ID (`evt_…`), the same on every attempt |
| `webhook-timestamp` | Unix seconds of this attempt |
| `webhook-signature` | `v1a,<base64 Ed25519 signature>` over `<webhook-id>.<webhook-timestamp>.<body>` |

- **Success:** any 2xx within 10 s. Redirects are not followed and count as failure.
- **Retries:** after attempt *n* fails, the next one follows after min(30 s · 2^(n−1), 1 h). An attempt due more than 24 h after the event is not sent; the delivery is given up.
- **At least once:** a control plane restart during a request sends the event again. Deduplicate by `webhook-id`.
- **No ordering:** events can arrive in any order, also for the same run. Order by `timestamp` or match by `data.run.id`.
- **Independent receivers:** each subscription has its own retries; a dead receiver does not hold back others.
- **No effect on runs:** a failing or slow receiver never delays provisioning, teardown or any state change. At most 8 requests are in flight per instance.

```mermaid
sequenceDiagram
    participant S as Scheduler / agent channel
    participant DB as PostgreSQL
    participant D as Outbox dispatcher (leader)
    participant R as Receiver

    S->>DB: BEGIN, state change (for example mark_started)
    S->>DB: INSERT lifecycle_events (envelope, built once)
    S->>DB: INSERT outbox job per matching subscription
    S->>DB: COMMIT, NOTIFY outbox
    DB-->>D: NOTIFY outbox (fallback: 5 s poll)
    D->>DB: claim due jobs (FOR UPDATE SKIP LOCKED), set lease; give up jobs older than 24 h
    D->>D: project subscription: resolve host, refuse internal addresses
    D->>D: sign id.timestamp.body
    D->>R: POST envelope with webhook-* headers (10 s timeout)
    alt 2xx
        R-->>D: 2xx
        D->>DB: job done, subscription last status
    else error, timeout, non-2xx or refused address
        D->>DB: next attempt after backoff, last error
    end
```

An event is recorded in the same transaction as the state change: a rolled-back change sends nothing, a committed one is delivered even if the control plane restarts first.

## Verifying signatures

Verify every delivery with the instance public key before trusting the body. Reject attempts whose `webhook-timestamp` is more than a few minutes off to limit replays; retries carry a fresh timestamp. `webhook-signature` may hold several space-separated signatures; accept the request if one verifies.

Python ([`cryptography`](https://cryptography.io/)):

```python
import base64
import json
import time

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

TOLERANCE_SECONDS = 300


def verify(public_key: str, headers: dict[str, str], body: bytes) -> dict:
    """Returns the event if the delivery is signed by `public_key` (`whpk_…`), else raises."""
    key = Ed25519PublicKey.from_public_bytes(base64.b64decode(public_key.removeprefix("whpk_")))
    msg_id = headers["webhook-id"]
    timestamp = headers["webhook-timestamp"]
    if abs(time.time() - int(timestamp)) > TOLERANCE_SECONDS:
        raise ValueError("webhook-timestamp outside the tolerance")
    message = f"{msg_id}.{timestamp}.".encode() + body
    for signature in headers["webhook-signature"].split(" "):
        version, _, value = signature.partition(",")
        if version != "v1a":
            continue
        try:
            key.verify(base64.b64decode(value), message)
        except InvalidSignature:
            continue
        return json.loads(body)
    raise ValueError("no valid signature")
```

Go (standard library):

```go
package webhook

import (
	"crypto/ed25519"
	"encoding/base64"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"
)

const tolerance = 5 * time.Minute

// Verify returns nil if the delivery is signed by publicKey (`whpk_…`).
func Verify(publicKey string, header http.Header, body []byte) error {
	key, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(publicKey, "whpk_"))
	if err != nil || len(key) != ed25519.PublicKeySize {
		return errors.New("invalid public key")
	}
	id, timestamp := header.Get("webhook-id"), header.Get("webhook-timestamp")
	seconds, err := strconv.ParseInt(timestamp, 10, 64)
	if err != nil {
		return errors.New("invalid webhook-timestamp")
	}
	if age := time.Since(time.Unix(seconds, 0)); age > tolerance || age < -tolerance {
		return errors.New("webhook-timestamp outside the tolerance")
	}
	message := append([]byte(id+"."+timestamp+"."), body...)
	for _, signature := range strings.Fields(header.Get("webhook-signature")) {
		version, value, ok := strings.Cut(signature, ",")
		if !ok || version != "v1a" {
			continue
		}
		raw, err := base64.StdEncoding.DecodeString(value)
		if err == nil && ed25519.Verify(ed25519.PublicKey(key), message, raw) {
			return nil
		}
	}
	return errors.New("no valid signature")
}
```

Verify against the raw request body, before any JSON parsing or re-encoding.

## Key rotation

The key is stable: the chart creates it with cert-manager `rotationPolicy: Never`, and certificate renewal keeps it. Rotation is manual: delete the key Secret, restart the control plane, give receivers the new public key (steps: [chart README, "Lifecycle event signing key"](https://github.com/prskr/dark-factory/blob/main/deploy/helm/dark-factory/README.md#lifecycle-event-signing-key)). Until a receiver has the new key, its verification fails; the deliveries are retried for 24 h, so a receiver that updates within that window and answers non-2xx meanwhile loses nothing. Receivers can fetch `GET /lifecycle-events/public-key` to automate the update.

## Address filter

Project subscriptions are configured by project admins in the UI, so their targets are restricted:

- The URL must be `https://`. An IP literal in the URL must be public; this is checked on save and before each attempt.
- Before each attempt the host is resolved, and the attempt is refused (`address refused: …`) if **any** resolved address is loopback, private (RFC 1918), CGNAT (`100.64.0.0/10`), link-local (including `169.254.169.254`), unique-local (`fc00::/7`), unspecified, broadcast or documentation, or an IPv4-mapped IPv6 form of these. The checked address is the one connected to, so DNS rebinding does not get around it.
- Redirects are not followed, and no proxy from the environment is used.

Limits: the filter knows only these well-known ranges. It does not know a cluster's own public address ranges (pod or Service CIDRs outside the private ranges) or internal services reachable on public addresses. To restrict the control plane further, add an egress NetworkPolicy. The chart's control plane NetworkPolicy restricts ingress only. An egress policy must still allow everything else the control plane talks to (DNS, PostgreSQL, the Kubernetes API, the OIDC provider, forges, the gateway, instance receivers). Sketch:

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: dark-factory-control-plane-egress
  namespace: dark-factory
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/instance: dark-factory
      app.kubernetes.io/component: control-plane
  policyTypes: [Egress]
  egress:
    # ... rules for DNS, PostgreSQL, the Kubernetes API, the OIDC provider, the gateway ...
    - to:
        - ipBlock:
            cidr: 0.0.0.0/0
            except: [203.0.113.0/24] # internal ranges the address filter does not know
      ports:
        - {protocol: TCP, port: 443}
```

Instance subscriptions are trusted configuration and bypass the filter.

For development, a `dev-tools` build accepts `--unrestricted-project-subscriptions`, which lifts the restrictions for project subscriptions ([development.md](https://github.com/prskr/dark-factory/blob/main/docs/development.md)). Release builds don't have the flag.
