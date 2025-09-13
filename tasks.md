# arRPC Rust Roadmap (Precise)

Milestone-based plan to reach Discord Local RPC parity with a focused, testable backlog. Reference: <https://discord.com/developers/docs/topics/rpc>

---

## M0. Foundations

- [x] Workspace crates: drpc-core, drpc-ipc, drpc-ws, drpc-bridge, drpc-process, drpc.
- [x] Protocol models: IpcOp, RpcCommand, IncomingFrame, OutgoingFrame, Ready{config,user}, Activity (+ timestamps/assets/buttons/flags).
- [x] Frame codec: 8-byte header + JSON, encode/decode + tests (round-trip, invalid op, truncated).
- [x] EventBus (publish/subscribe) for cross-subsystem fan-out.

Acceptance

- `cargo test -p drpc-core` passes, including frame tests.

---

## M1. Transports

- [x] IPC (Unix): discover discord-ipc-0..9 under XDG_RUNTIME_DIR|TMP\*|/tmp; remove stale; handshake → READY; PING/PONG; SET_ACTIVITY.
- [x] IPC (Windows): named pipe path selection scaffold \\?\\pipe\\discord-ipc-<n> (full accept loop TBD).
- [x] WS RPC: port scan 6463–6472; origin allowlist; v=1&encoding=json&client_id validation; READY on connect; SET_ACTIVITY.
- [x] Payload size guard (64 KiB) on WS + IPC.

Acceptance

- Manually connect via ws client; see READY then ACTIVITY_UPDATE on SET_ACTIVITY.
- IPC unit test covers handshake + READY.

---

## M2. Bridge + Fan-out

- [x] Bridge WS on 127.0.0.1 with configurable port; maintain last activity per socketId and replay on connect.
- [x] Subscribe to EventBus; broadcast live updates to connected Bridge clients.

Acceptance

- Open Bridge; after RPC SET_ACTIVITY, activity is broadcast and replayed on reconnect.

---

## M3. Detectables + Scanning

- [x] Detectables loader to ~/.drpc/detectables.json with TTL and optional refresh; async network fetch with retries; logs age/ttl.
- [x] Process scanning crate scaffold with backend trait; Linux placeholder backend; diff-and-publish loop.
- [x] Matching algorithm parity (path variants, launcher skips, java arg rules) and tests.

Acceptance

- drpc creates/refreshes detectables file; logs count and age.
- Scanner task runs (can be disabled); emits CLEAR when PID disappears (once backend implemented).

---

## M4. Commands Parity

- [x] SET_ACTIVITY: normalize timestamps (s→ms), map instance→flags bit0, map buttons→metadata.button_urls + label list; emit ACTIVITY_UPDATE.
- [x] CONNECTIONS_CALLBACK: respond { evt: ERROR, data: { code: 1000 } }.
- [x] INVITE_BROWSER/GUILD_TEMPLATE_BROWSER/DEEP_LINK: placeholder ACK, extensible for callbacks.
- [x] CLEAR activity on transport disconnect and persist per-socket registry.

Acceptance

- Commands above return structures compatible with existing clients.

---

## M5. CLI & Config

- [x] CLI flags: --no-process-scanning, --bridge-port, --refresh-detectables, --detectables-ttl, --log-format {pretty|json}, --config.
- [x] Env: DRPC_NO_PROCESS_SCANNING, DRPC_DEBUG.
- [x] ~/.drpc/config.toml overrides defaults; CLI wins.

Acceptance

- Flags and config load; JSON logging selectable; process scanning toggles.

---

## M6. Observability & Hardening

- [x] Origin checks for WS; Bridge bound to localhost.
- [x] Payload caps (64 KiB) with close.
- [x] Connection spans keyed by socketId for logs.
- [x] Graceful shutdown: broadcast CLEAR for all active sockets.

Acceptance

- Ctrl+C logs shutdown and (once implemented) emits CLEARs.

---

## M7. Tests

- [x] Core codec tests (round-trip, invalid op, truncated).
- [x] Activity normalization test (buttons metadata present).
- [x] WS integration test (READY then ACTIVITY_UPDATE).
- [x] IPC integration test (handshake, SET_ACTIVITY).
- [x] Detectables loader test (TTL, parse fallback).
- [ ] Process matcher tests.
  - Added additional matcher coverage (extensions, java fallback, launcher skip).
- [ ] End-to-end test (WS -> Bridge replay).

Acceptance

- `cargo test --all` passes with targeted features enabled.

---

## M8. Cross-Platform & Packaging

- [x] Windows named pipe server accept/read/write parity with Unix.
- [x] macOS/Linux/Windows build guidance (CI later).
- [x] Optional --print-socket-paths for debug.

---

## Stretch / Backlog

- [ ] Erlpack/ETF encoding negotiation.
  - scaffolded: accepts encoding=etf (fallback to JSON); full binary pending.
- [ ] HTTP REST surface mirroring local RPC.
- [ ] Live detectables hot-reload endpoint.
- [ ] Activity privacy filters (allow/deny).
- [ ] Metrics (feature metrics): active_connections, activities_set, processes_detected, detectables_count.

---

## Definition of Done

- [ ] drpc reproduces local RPC behavior (IPC/WS): client connects, sets activity, Bridge shows status; clears on disconnect.
- [ ] Command responses match Node semantics for supported set.
- [ ] Detectables file managed; scanning updates activities periodically (or disabled by flag).
- [ ] Logging structured; debug mode provides frame-level insight.
- [ ] Repo includes this roadmap and updated README.

Reference

- Discord RPC docs: <https://discord.com/developers/docs/topics/rpc>
