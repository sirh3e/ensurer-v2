# Configuration

Both binaries (`runsd` and `runs`) use a three-layer configuration stack.
Later layers override earlier ones:

1. Built-in defaults (always present, no file required)
2. TOML config file (optional)
3. Environment variables (highest priority)

---

## `runs` — TUI client

### Changing the server endpoint

The client connects to `runsd` via a **Unix domain socket** (macOS / Linux) or a
**TCP port** (Windows).  The socket / port must match what `runsd` is listening on.

#### Unix / macOS (socket path)

| Method | How |
|--------|-----|
| CLI flag | `runs --socket /run/user/1000/runsd.sock` |
| Env var | `RUNSD_SOCKET=/run/user/1000/runsd.sock runs` |
| Config file | `socket_path = "/run/user/1000/runsd.sock"` |

Default (when nothing is set): `$XDG_RUNTIME_DIR/runsd.sock`, falling back to
`/tmp/runsd.sock`.

#### Windows (TCP port)

| Method | How |
|--------|-----|
| CLI flag | `runs --port 5000` |
| Env var | `RUNSD_PORT=5000 runs` |
| Config file | `port = 5000` |

Default: `4242`.

### Config file

Location (evaluated in order, first one wins):

| Platform | Path |
|----------|------|
| Linux / macOS | `$XDG_CONFIG_HOME/runs/config.toml` → `~/.config/runs/config.toml` |
| Windows | `%APPDATA%\runs\config.toml` |

Print the default config to stdout and use it as a starting point:

```sh
runs --init-config > ~/.config/runs/config.toml
```

Full file reference:

```toml
# Unix/macOS: path to the runsd Unix socket.
# Uncomment and edit to override the default ($XDG_RUNTIME_DIR/runsd.sock).
# socket_path = "/run/user/1000/runsd.sock"

# Windows: TCP port of the runsd server.
# port = 4242

# How many runs to load per page (max 100).
page_size = 100
```

### Environment variable reference (`runs`)

| Variable | Config key | Description |
|----------|------------|-------------|
| `RUNSD_SOCKET` | `socket_path` | Unix socket path (Unix only) |
| `RUNSD_PORT` | `port` | TCP port (Windows only) |
| `RUNS_PAGE_SIZE` | `page_size` | Runs per page |

---

## `runsd` — daemon

### Changing what the daemon listens on

If you move the socket (or port), update the `runs` client to match using one
of the methods above.

#### Unix / macOS (socket path)

| Method | How |
|--------|-----|
| CLI flag | `runsd --socket /run/user/1000/runsd.sock` |
| Env var | `RUNSD_SERVER_SOCKET_PATH=/run/user/1000/runsd.sock runsd` |
| Config file | `[server]` → `socket_path = "/run/user/1000/runsd.sock"` |

Default: `$XDG_RUNTIME_DIR/runsd.sock` → `/tmp/runsd.sock`.

#### Windows (TCP port)

| Method | How |
|--------|-----|
| CLI flag | `runsd --port 5000` |
| Env var | `RUNSD_SERVER_PORT=5000 runsd` |
| Config file | `[server]` → `port = 5000` |

Default: `4242`.

### Config file

Location (evaluated in order, first one wins):

| Platform | Path |
|----------|------|
| Linux / macOS | `$XDG_CONFIG_HOME/runsd/config.toml` → `~/.config/runsd/config.toml` |
| Windows | `%APPDATA%\runsd\config.toml` |

Override with: `runsd --config /path/to/config.toml`

Print the default config to stdout:

```sh
runsd --init-config > ~/.config/runsd/config.toml
```

Full file reference with all defaults:

```toml
[server]
# Unix domain socket path (ignored on Windows).
socket_path = "/run/user/1000/runsd.sock"   # default: $XDG_RUNTIME_DIR/runsd.sock
# TCP port (ignored on Unix).
port = 4242
# Directory where result files are stored.
data_dir = "~/.local/share/runsd"
# Maximum number of calculations that run simultaneously.
max_concurrent_calculations = 8

[retry]
# Delay before the first retry (milliseconds).
base_delay_ms = 1000
# Upper bound on retry delay (milliseconds).
max_delay_ms = 300000
# How many times a calculation is retried before it is marked failed.
max_attempts = 5

[lease]
# How often a running calculation renews its lease (seconds).
heartbeat_interval_s = 10
# A lease is considered expired after this many seconds without a heartbeat.
expiry_s = 60
# How often the watchdog scans for expired leases (seconds).
watchdog_interval_s = 30

[external_api]
# Base URL of the external calculation API.
base_url = "https://example.com"
# Per-request timeout (seconds).
request_timeout_s = 30
# Whether the external API supports idempotency keys.
supports_idempotency = true

[logging]
# Log level written to the log file (trace/debug/info/warn/error).
file_level = "info"
# Log level written to stderr.
stderr_level = "warn"
# Log file location.
file_path = "~/.local/state/runsd/runsd.log"
# Delete SSE events older than this many days (0 = keep forever).
event_retention_days = 30
```

### Environment variable reference (`runsd`)

Variables follow the pattern `RUNSD_<SECTION>_<KEY>` (all upper-case,
underscores separate nesting levels).

| Variable | Config path | Description |
|----------|-------------|-------------|
| `RUNSD_SERVER_SOCKET_PATH` | `server.socket_path` | Unix socket path |
| `RUNSD_SERVER_PORT` | `server.port` | TCP port (Windows) |
| `RUNSD_SERVER_DATA_DIR` | `server.data_dir` | Result file directory |
| `RUNSD_SERVER_MAX_CONCURRENT_CALCULATIONS` | `server.max_concurrent_calculations` | Worker concurrency |
| `RUNSD_RETRY_BASE_DELAY_MS` | `retry.base_delay_ms` | Initial retry delay |
| `RUNSD_RETRY_MAX_DELAY_MS` | `retry.max_delay_ms` | Max retry delay |
| `RUNSD_RETRY_MAX_ATTEMPTS` | `retry.max_attempts` | Max retry count |
| `RUNSD_LEASE_HEARTBEAT_INTERVAL_S` | `lease.heartbeat_interval_s` | Heartbeat period |
| `RUNSD_LEASE_EXPIRY_S` | `lease.expiry_s` | Lease TTL |
| `RUNSD_LEASE_WATCHDOG_INTERVAL_S` | `lease.watchdog_interval_s` | Watchdog scan period |
| `RUNSD_EXTERNAL_API_BASE_URL` | `external_api.base_url` | External API base URL |
| `RUNSD_EXTERNAL_API_REQUEST_TIMEOUT_S` | `external_api.request_timeout_s` | Request timeout |
| `RUNSD_EXTERNAL_API_SUPPORTS_IDEMPOTENCY` | `external_api.supports_idempotency` | Idempotency support |
| `RUNSD_LOGGING_FILE_LEVEL` | `logging.file_level` | File log level |
| `RUNSD_LOGGING_STDERR_LEVEL` | `logging.stderr_level` | Stderr log level |
| `RUNSD_LOGGING_FILE_PATH` | `logging.file_path` | Log file path |
| `RUNSD_LOGGING_EVENT_RETENTION_DAYS` | `logging.event_retention_days` | Event pruning age |

---

## Quick-start: custom socket path

If `runsd` is running on a non-default socket, update both sides:

```sh
# Start the daemon on a custom socket
runsd --socket /tmp/my-runsd.sock

# Connect the TUI client to the same socket
runs --socket /tmp/my-runsd.sock

# Or set it permanently in the client config
echo 'socket_path = "/tmp/my-runsd.sock"' >> ~/.config/runs/config.toml
```

Verify connectivity with the built-in doctor command:

```sh
runs --socket /tmp/my-runsd.sock doctor
```

---

## GraphQL API

`runsd` also exposes a GraphQL endpoint at `POST /graphql`.
The interactive Playground is available at `GET /graphql` in a browser.

The endpoint is served on the same socket / port as the REST API — no
separate listener or config key is needed.
