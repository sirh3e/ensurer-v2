# External API client

`runsd` calls an external HTTP service to execute calculations.
This document describes the contract that service must implement and how
to configure the client.

---

## Configuration

All settings live under `[external_api]` in `~/.config/runsd/config.toml`
(or the file passed to `runsd --config`).

```toml
[external_api]
base_url             = "https://api.example.com"
request_timeout_s    = 30
supports_idempotency = true
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `base_url` | string | `https://example.com` | Root URL — no trailing slash |
| `request_timeout_s` | integer | `30` | Per-request timeout in seconds |
| `supports_idempotency` | bool | `true` | Send `Idempotency-Key` header |

Environment variable equivalents (override the file):

```sh
RUNSD_EXTERNAL_API_BASE_URL=https://api.example.com
RUNSD_EXTERNAL_API_REQUEST_TIMEOUT_S=30
RUNSD_EXTERNAL_API_SUPPORTS_IDEMPOTENCY=true
```

Print the full default config: `runsd --init-config`

See [`config/runsd.toml`](../config/runsd.toml) for a ready-to-copy template.

---

## HTTP contract

### Request

```
POST {base_url}/calculations/{kind}
Content-Type: application/json
Idempotency-Key: {idempotency_key}   ← only when supports_idempotency = true

{input_json}
```

| Part | Description |
|------|-------------|
| `{kind}` | The calculation type submitted by the caller (e.g. `pricing`, `risk`) |
| `{input_json}` | Arbitrary JSON object supplied when the run was submitted |
| `{idempotency_key}` | Stable SHA-256 hex derived from `kind` + canonical input — same input always produces the same key |

### Response — success (`2xx`)

Return any `2xx` status. The response body is written verbatim to disk at:

```
{data_dir}/results/{calc_id}/result.json
```

The result can be retrieved via the REST API:
`GET /calculations/{calc_id}/result`

### Response — transient error (will be retried)

Return any of:

| Status | Meaning |
|--------|---------|
| `408 Request Timeout` | Request took too long |
| `429 Too Many Requests` | Rate limited |
| `5xx` | Server-side error |

`runsd` will retry with exponential back-off up to `retry.max_attempts` times
(default 5). Each retry uses the same idempotency key so the external service
can de-duplicate safely.

### Response — permanent error (not retried)

Any other non-`2xx` status (e.g. `400 Bad Request`, `422 Unprocessable Entity`)
marks the calculation as permanently failed.

---

## Retry behaviour

Back-off is jittered exponential, configured under `[retry]`:

```toml
[retry]
base_delay_ms = 1000    # delay before attempt 2
max_delay_ms  = 300000  # cap (5 minutes)
max_attempts  = 5       # attempts 1–5; failure on attempt 6
```

The delay for attempt *n* is a random value in `[0, min(base × 2^(n-1), max)]`.

| Attempt | Max delay |
|---------|-----------|
| 1 | immediate |
| 2 | 1 s |
| 3 | 2 s |
| 4 | 4 s |
| 5 | 8 s |
| … | … capped at `max_delay_ms` |

After all attempts are exhausted the calculation status becomes `failed`
with `error_kind = transient_exhausted`.

---

## Idempotency key

When `supports_idempotency = true` every request carries:

```
Idempotency-Key: <64-char hex string>
```

The key is a SHA-256 hash of `kind` + the **canonical** (sorted-key) JSON
representation of `input_json`. The same logical calculation always produces
the same key regardless of how the JSON was originally formatted.

If your API does not support idempotency keys, set:

```toml
[external_api]
supports_idempotency = false
```

The header will not be sent.

---

## Result storage

On success the raw response body is stored at:

```
{server.data_dir}/results/{calc_id}/result.json
```

`data_dir` defaults to `~/.local/share/runsd`. Override it:

```toml
[server]
data_dir = "/var/lib/runsd"
```

---

## Example: minimal compliant server (pseudo-code)

```
POST /calculations/pricing
Body: { "symbol": "AAPL", "quantity": 100 }
Idempotency-Key: 3a7f...

→ 200 OK
Body: { "price": 182.50, "currency": "USD" }
```

```
POST /calculations/risk
Body: { "portfolio_id": "abc123" }

→ 503 Service Unavailable      ← runsd retries
→ 503 Service Unavailable      ← runsd retries
→ 200 OK
Body: { "var_95": 0.042 }
```
