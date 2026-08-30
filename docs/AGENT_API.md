# MDL Agent API Reference (v26.3)

Programmatic control surface for AI agents and automation tooling.

> **Authoritative source**: `mdl capabilities --format json`
> (schema `mdl.capabilities/v1`) always reflects the running binary.
> This document mirrors it for humans; on conflict, trust the manifest.

## Quick Start

```bash
mdl agent --port 8080            # loopback HTTP+WebSocket control plane
mdl capabilities --format json   # discover everything without reading this file
```

All endpoints bind `127.0.0.1` by default (`--bind` to override). No auth
token mechanism exists — do not expose the port beyond localhost.

## REST Endpoints

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/v1/status` | Launcher version, uptime, running instances |
| GET | `/api/v1/capabilities` | Machine-readable feature manifest |
| POST | `/api/v1/execute` | Run an execute command (below) |
| GET | `/api/v1/events` | WebSocket event stream |
| GET | `/api/v1/game/windows` | Visible Minecraft game windows |
| GET | `/api/v1/game/:instance/status` | In-game state via Despotes |
| GET | `/api/v1/game/:instance/screenshot` | PNG (framebuffer, WGC fallback; `?base64=true` for JSON-wrapped) |
| GET | `/api/v1/game/:instance/ready` | 200 when in-world/menu-ready, else 503 |
| GET | `/api/v1/game/:instance/idle-status` | Idle watchdog: last-output age, threshold, fired? |
| POST | `/api/v1/game/:instance/input` | Inject key/look/click/scroll/chat + v26.9 automation (schedule/macro/condition/raw-action) |
| POST | `/api/v1/game/:instance/redstone` | Redstone signal query at a block position (no body = crosshair probe) |
| GET | `/api/v1/instance/:instance/metrics` | Launch metrics; `?history=true` for all records |
| GET | `/api/v1/instance/:instance/disk` | Disk usage + top-level breakdown |

### v26.9 Automation Inputs (`POST /api/v1/game/:instance/input`)

Beyond key/look/click/scroll/chat, the input endpoint accepts the Despotes
v26.9 automation primitives:

```jsonc
// schedule: named repeating action sequence (client-thread ticked)
{"type":"schedule","op":"add","name":"heartbeat","periodTicks":100,
 "commands":[{"type":"chat","text":"hi"}]}
{"type":"schedule","op":"status"}
{"type":"schedule","op":"remove","name":"heartbeat"}

// macro: record & replay, one tick between steps on replay
{"type":"macro","op":"start-recording","name":"demo"}
{"type":"macro","op":"record-step","name":"demo",
 "step":{"type":"look","yaw":90,"pitch":0}}
{"type":"macro","op":"stop-recording"}
{"type":"macro","op":"play","name":"demo"}

// condition: dot-path field extraction + exists/eq/ne/gt/lt/contains
{"type":"condition",
 "if":{"type":"status","field":"result.inGame","op":"exists"},
 "then":[{"type":"ping"}],
 "else":[{"type":"chat","text":"not in game"}]}

// raw-action: forward-compatible protocol passthrough
{"type":"raw-action","command":{"type":"ping"}}
```

Redstone query (`POST /api/v1/game/:instance/redstone`): body optional —
`{"x":..,"y":..,"z":..}` probes a block; an empty body probes the crosshair
target block. Returns the block, max incoming signal and adjacent components.

### Execute Commands (`POST /api/v1/execute`)

Body: `{"command":"…","args":[…],"options":{…}}`.
Response: `{"status","exit_code","stdout","error_code?","data?"}`.

| Command | Args | Notable options |
|---|---|---|
| `list` | – | – |
| `create` | `<name> [version]` | – |
| `info` | `<name>` | – |
| `launch` | `<name>` | username, server, fullscreen, width, height, agent, agent-port, java-path, **jdk aprism[@ver]**, memory, aprism, enter-test-world, no-queue, idle-timeout, no-idle-timeout, **oom-confirm auto\|always\|never**, **oom-list-only**, javaagents |
| `stop` | `<name>` | – |
| `metrics` | `<name>` | history=true |
| `disk` | `<name>` | – |
| `inject-agent` | `<name> <jar>` | params, java-path |
| `server-cmd` | `<server> <command…>` | – |

### Error shape (v26.5-alpha.2)

Every client error on this API returns the JSON envelope
`{"status":"error","error":"…"}` — including JSON-body extraction
rejections (400), automation-input validation failures (400: unknown
op, missing required fields), malformed `/redstone` bodies (400) and
`/execute` failures (carrying machine-readable `error_code`). Only
upstream Despotes failures map to 502. Bodies over the default limit
return 413; unknown instances return 404.

## WebSocket Events (`GET /api/v1/events`)

Every frame: JSON object with `type` + `timestamp`.

| kind | payload highlights |
|---|---|
| `launch_started` | instance |
| `launch_progress` | stage, progress, message |
| `launch_completed` | pid |
| `launch_failed` | error |
| `log_line` | level, message |
| `instance_stopped` | exit_code |
| `game_ready` | pid, in_world |
| `game_idle_timeout` | pid, idle_seconds |

## Error Codes

`POST /execute` failures carry `error_code`; treat unknown codes as INTERNAL.

| code | HTTP | meaning |
|---|---|---|
| UNKNOWN_COMMAND | 400 | unrecognized execute name |
| BAD_REQUEST | 400 | missing/invalid argument |
| NOT_FOUND | 404 | instance/server does not exist |
| ALREADY_EXISTS | 409 | duplicate create/rename target |
| NOT_RUNNING | 409 | stop/cmd against non-running target |
| BUSY | 409 | launch lock held elsewhere |
| GONE | 410 | process exited before query |
| INTERNAL | 500 | unclassified |
| NOT_IMPLEMENTED | 501 | platform-gated (e.g. screenshots off-Windows) |
| BAD_GATEWAY | 502 | upstream (Despotes/RCON) failure |
| SERVICE_UNAVAILABLE | 503 | Despotes/game not reachable yet |

## Minimal Recipes

```bash
# Discover + launch + await readiness
curl -s localhost:8080/api/v1/capabilities | jq .
curl -s -X POST localhost:8080/api/v1/execute \
  -H 'Content-Type: application/json' \
  -d '{"command":"launch","args":["my-instance"],
       "options":{"agent":"true","wait-ready":"true",
                  "oom-confirm":"never","idle-timeout":"300"}}'
curl -s localhost:8080/api/v1/game/my-instance/ready
```

```python
import json, asyncio, websockets, urllib.request

def post(cmd, args, opts=None):
    body = json.dumps({"command": cmd, "args": args, "options": opts or {}}).encode()
    req = urllib.request.Request("http://127.0.0.1:8080/api/v1/execute", body,
                                 {"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req))

async def events():
    async with websockets.connect("ws://127.0.0.1:8080/api/v1/events") as ws:
        async for msg in ws:
            ev = json.loads(msg)
            print(f"[{ev['type']}]", ev.get("message") or ev.get("instance", ""))

post("launch", ["my-instance"], {"detach-implicit": "always-detached"})
asyncio.run(events())
```

```javascript
// Node 18+: fetch + WebSocket (undici global WebSocket in Node 21+, or 'ws')
const base = "http://127.0.0.1:8080";
const post = async (command, args = [], options = {}) =>
  (await fetch(`${base}/api/v1/execute`, {
    method: "POST", headers: {"Content-Type": "application/json"},
    body: JSON.stringify({command, args, options}),
  })).json();

await post("server-cmd", ["my-server", "whitelist", "add", "TestBot"]);
const disk = await (await fetch(`${base}/api/v1/instance/my-instance/disk`)).json();
console.log(disk.data.human_total);
```

## See Also

- `docs/specification.md` — broader CLI/config specification
- `examples/agent_demo.py` / `examples/agent_demo.js` — runnable walkthroughs
- Upstream README — CLI command catalog
