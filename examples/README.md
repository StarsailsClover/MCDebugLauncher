# Agent API Examples

Runnable walkthroughs of the MDL Agent API (v26.3 surface). Both demos
follow the same arc: capability discovery → launch with explicit safety
options → game-ready event loop → in-game input → idle-status / metrics /
disk observability → graceful stop.

> The machine-authoritative surface is always
> `mdl capabilities --format json`; these examples are illustrative.

## Prerequisites

```bash
mdl agent --port 8080                 # control plane on loopback
mdl create demo --mc-version 26.2 --loader fabric   # instance with Despotes
```

## Python (`agent_demo.py`)

```bash
pip install websockets
python agent_demo.py
```

- stdlib-only HTTP (urllib), `websockets` for the event stream
- shows `game_ready` correlation and failure fallback to metrics

## Node.js (`agent_demo.js`)

```bash
node agent_demo.js        # Node 18+ fetch; WebSocket needs Node 21+, or npm i ws
```

## Endpoints exercised

| Step | Endpoint / command |
|---|---|
| discovery | `GET /api/v1/capabilities` |
| launch | execute `launch` with `agent`, `oom-confirm`, `idle-timeout` |
| readiness | WS `/api/v1/events` → `game_ready` |
| input | `POST /api/v1/game/:i/input` (chat) |
| idle watchdog | `GET /api/v1/game/:i/idle-status` |
| metrics | `GET /api/v1/instance/:i/metrics` |
| disk | `GET /api/v1/instance/:i/disk` |
| stop | execute `stop` |

Full reference: [../docs/AGENT_API.md](../docs/AGENT_API.md).
