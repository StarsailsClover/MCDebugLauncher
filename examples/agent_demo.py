#!/usr/bin/env python3
"""MDL Agent API walkthrough (v26.3).

Demonstrates: capability discovery, launch with safety options, the
game-ready event loop, in-game input, idle-status polling, metrics and
disk queries, and a graceful stop.

Requirements: `pip install websockets`, mdl agent running on :8080,
an instance named `demo` created with Despotes.
"""

import asyncio
import json
import urllib.request

BASE = "http://127.0.0.1:8080"


def api_get(path: str):
    with urllib.request.urlopen(f"{BASE}{path}") as r:
        return json.load(r)


def execute(command: str, args=None, options=None):
    body = json.dumps(
        {"command": command, "args": args or [], "options": options or {}}
    ).encode()
    req = urllib.request.Request(
        f"{BASE}/api/v1/execute", body, {"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req) as r:
        return json.load(r)


async def wait_game_ready(ws, timeout_s=180):
    """Resolve on the first game_ready frame for any instance."""
    try:
        return await asyncio.wait_for(_ready_loop(ws), timeout_s)
    except (asyncio.TimeoutError, TimeoutError):
        return False


async def _ready_loop(ws):
    async for msg in ws:
        ev = json.loads(msg)
        print(f"  [event] {ev['type']}: {ev.get('instance', '')}")
        if ev["type"] == "game_ready":
            return True
        if ev["type"] in ("launch_failed", "game_idle_timeout"):
            return False


async def main():
    # 1. Discover what this binary can do (schema mdl.capabilities/v1).
    caps = api_get("/api/v1/capabilities")
    print(f"MDL {caps['version']} — {len(caps['endpoints'])} endpoints, "
          f"{len(caps['execute_commands'])} execute commands")

    instance = "demo"

    # 2. Launch detached with agent control; OOM confirmation policy is
    #    explicit so behavior is identical in interactive and CI shells.
    result = execute("launch", [instance], {
        "agent": "true",
        "oom-confirm": "never",
        "idle-timeout": "300",
    })
    print("launch:", result["status"])

    # 3. Follow events until the game broadcasts readiness.
    import websockets
    async with websockets.connect(f"ws://127.0.0.1:8080/api/v1/events") as ws:
        ready = await wait_game_ready(ws)
        print("ready:", ready)

    if not ready:
        print("launch did not reach ready state — dumping diagnostics hints")
        diag = api_get(f"/api/v1/instance/{instance}/metrics")
        print(json.dumps(diag["data"]["launches"][-1:], indent=2))
        return

    # 4. In-game observation + one injected action.
    status = api_get(f"/api/v1/game/{instance}/status")
    print("inGame:", status.get("inGame"))
    execute_input = {
        "type": "chat",
        "message": "hello from MDL agent demo"
    }
    req = urllib.request.Request(
        f"{BASE}/api/v1/game/{instance}/input",
        json.dumps(execute_input).encode(),
        {"Content-Type": "application/json"},
    )
    urllib.request.urlopen(req)

    # 5. Observability: idle watchdog + launch metrics + disk usage.
    print(json.dumps(api_get(f"/api/v1/game/{instance}/idle-status"), indent=2))
    metrics = api_get(f"/api/v1/instance/{instance}/metrics")
    print("last launch:", metrics["data"]["launches"][-1] if metrics["data"]["launches"] else None)
    disk = api_get(f"/api/v1/instance/{instance}/disk")
    print("disk:", disk["data"]["human_total"])

    # 6. Graceful stop.
    print(execute("stop", [instance])["stdout"])


if __name__ == "__main__":
    asyncio.run(main())
