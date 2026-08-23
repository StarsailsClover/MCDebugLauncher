#!/usr/bin/env node
/**
 * MDL Agent API walkthrough (v26.3) — Node 18+.
 *
 * Demonstrates: capability discovery, launch with safety options, the
 * game-ready event loop, in-game input, idle-status polling, metrics and
 * disk queries, and a graceful stop. Uses the global fetch (Node 18+) and
 * global WebSocket (Node 21+; on older Node `npm i ws` and swap in).
 */

const BASE = "http://127.0.0.1:8080";

async function apiGet(path) {
  const r = await fetch(`${BASE}${path}`);
  if (!r.ok && r.status !== 503) throw new Error(`${path} -> ${r.status}`);
  return r.json();
}

async function execute(command, args = [], options = {}) {
  const r = await fetch(`${BASE}/api/v1/execute`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ command, args, options }),
  });
  return r.json();
}

/** Resolve true on first game_ready frame; false on failure/idle-timeout. */
function waitGameReady(ws, timeoutMs = 180000) {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), timeoutMs);
    const onMsg = (raw) => {
      const ev = JSON.parse(raw.data ?? raw);
      console.log(`  [event] ${ev.type}: ${ev.instance ?? ""}`);
      if (ev.type === "game_ready") { clearTimeout(timer); resolve(true); }
      if (ev.type === "launch_failed" || ev.type === "game_idle_timeout") {
        clearTimeout(timer); resolve(false);
      }
    };
    ws.addEventListener("message", onMsg);
  });
}

async function main() {
  // 1. Capability discovery — never hardcode what you can query.
  const caps = await apiGet("/api/v1/capabilities");
  console.log(`MDL ${caps.version} — ${caps.endpoints.length} endpoints, ` +
              `${caps.execute_commands.length} execute commands`);

  const instance = "demo";

  // 2. Launch detached with agent control + explicit safety policy.
  const launch = await execute("launch", [instance], {
    agent: "true",
    "oom-confirm": "never",
    "idle-timeout": "300",
  });
  console.log("launch:", launch.status);

  // 3. Event loop until ready.
  const ws = new WebSocket(`ws://127.0.0.1:8080/api/v1/events`);
  await new Promise((res) => { ws.onopen = res; });
  const ready = await waitGameReady(ws);
  console.log("ready:", ready);
  if (!ready) {
    const m = await apiGet(`/api/v1/instance/${instance}/metrics`);
    console.log("last metrics:", m.data.launches.at(-1));
    process.exitCode = 1;
    return;
  }

  // 4. In-game observation + injected chat.
  const st = await apiGet(`/api/v1/game/${instance}/status`);
  console.log("inGame:", st.inGame);
  await fetch(`${BASE}/api/v1/game/${instance}/input`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ type: "chat", message: "hello from MDL agent demo" }),
  });

  // 5. Observability: idle watchdog, metrics, disk usage.
  console.log(await apiGet(`/api/v1/game/${instance}/idle-status`));
  const metrics = await apiGet(`/api/v1/instance/${instance}/metrics`);
  console.log("last launch:", metrics.data.launches.at(-1));
  const disk = await apiGet(`/api/v1/instance/${instance}/disk`);
  console.log("disk:", disk.data.human_total);

  // 6. Graceful stop.
  const stop = await execute("stop", [instance]);
  console.log("stop:", stop.stdout);

  ws.close();
}

main().catch((e) => { console.error(e); process.exit(1); });
