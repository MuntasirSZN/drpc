# arRPC Rust Rewrite Task List

Comprehensive plan for rewriting `@OpenAsar/arrpc` in pure Rust using the Axum ecosystem while preserving parity with the current NodeJS implementation and modernizing architecture. This document was produced after reviewing every file in the existing repository (JS sources, examples, extension scripts, detectable DB updater, changelog, etc.).

---

## 0. Objectives
- [ ] Pure Rust implementation (no Node dependency) of: IPC transport, WebSocket transport, bridge WebSocket, process scanning, activity dispatch, invite/template/deep-link command handling.
- [x] Use Axum (with Tokio) for all HTTP/WebSocket surfaces; implement IPC via Unix Domain Sockets / Windows Named Pipes.
- [ ] Structured logging (use `tracing` + env filter); `ARRPC_DEBUG=1` => debug level, otherwise info/warn minimal.
- [x] Generate / refresh `detectables.json` (renamed conceptually to `detectables.json`) once on startup if missing or stale; stored in `~/.arrpc/detectables.json` (create directory if needed). Support manual refresh flag. (placeholder fetch)
- [ ] Fix known issues from changelog & implicit bugs (see Section 11) and make behavior deterministic.
- [ ] Provide clean API surface (library crate) and binary (`arrpc`) with feature flags.
- [ ] Ensure compatibility with existing web bridge mods (same JSON payload structure) and clients using the original IPC protocol.
- [ ] Add tests for protocol framing, activity lifecycle, process matcher, detectables loader.

---

## 1. High-Level Architecture

Crate workspace structure:
- [x] `arrpc-core` (library): shared models, protocol enums, activity dispatcher, detectables DB, process matcher.
- [x] `arrpc-ipc` (feature `ipc`): IPC server implementation (Unix/Windows) replicating Discord IPC framing.
- [x] `arrpc-ws` (feature `ws`): RPC WebSocket server (ports 6463–6472 with origin + encoding validation; only JSON now).
- [x] `arrpc-bridge` (feature `bridge`): Bridge WebSocket (default port 1337 or env override `ARRPC_BRIDGE_PORT`).
- [x] `arrpc-process` (feature `process-scanning`): Process scanning abstraction and OS backends. (linux placeholder)
- [x] `arrpc-bin` (binary): CLI combining enabled features; config + runtime orchestration.

Runtime orchestration:
- [ ] Supervisor spawns enabled subsystems asynchronously; channels (Tokio broadcast + mpsc) propagate ActivityEvents.
- [ ] Activity normalization module sets timestamps, rewrites buttons, enforces JSON structure.
- [x] Event router fans out: (a) Bridge broadcast (all connected websockets); (b) Pending callbacks (invite/template/deep-link); (c) internal watchers. (bridge broadcasting implemented; callbacks pending)

---

## 2. Data Models / Protocol

Rust structs (serde-enabled) for frames:
- [x] `IpcOp` enum (Handshake, Frame, Close, Ping, Pong) mapping numeric codes (0–4).
- [x] `RpcCommand` enum: DISPATCH, SET_ACTIVITY, INVITE_BROWSER, GUILD_TEMPLATE_BROWSER, DEEP_LINK, CONNECTIONS_CALLBACK.
- [x] `IncomingFrame { cmd, args, nonce }` and `OutgoingFrame { cmd, data, evt, nonce }`.
- [x] READY payload struct with config + mock user (match existing values exactly for compatibility).
- [x] Activity model mirroring Node version (with optional `buttons`, `timestamps`, assets, instance flag → flags bit).
- [ ] Bridge message: `{ activity, pid, socketId }` as pass-through.

Timestamp handling:
- [x] Detect seconds vs milliseconds (heuristic: if digits length < current_ms_length - 2 → multiply by 1000).

---

## 3. IPC Transport (Discord-Compatible)
Tasks:
1. Determine usable socket path(s):
   - [x] Unix: iterate `XDG_RUNTIME_DIR`, `TMPDIR`, `TMP`, `TEMP`, fallback `/tmp`, names `discord-ipc-0..9`.
   - [ ] Windows: named pipe prefix `\\?\pipe\discord-ipc-<n>`.
2. For each candidate: test availability by connecting and performing PING (like JS `socketIsAvailable`). If remote responds appropriately (PONG / handshake), continue to next index. If stale socket (no reply / parse error) remove (Unix only) then claim path.
3. Implement frame encoding: 8-byte header (little endian: op(int32), len(int32)) + UTF-8 JSON body.
4. Handle handshake gating: first op must be Handshake or close with INVALID_VERSION / INVALID_CLIENTID.
5. Maintain `client_id`, `_handshook` flag, and assign internal socket ID.
6. Close codes mapping (export constants) mirroring Node enumerations.
7. Debug logging of frames when `ARRPC_DEBUG`.
8. Propagate messages to event router.
9. Tests: round-trip encode/decode, handshake rejection cases.

---

## 4. WebSocket RPC Server
Using Axum + `axum::extract::WebSocketUpgrade` + `tokio_tungstenite` behind the scenes.
Tasks:
1. Port scan 6463–6472; choose first free (bind test). Log chosen.
2. Accept only origins in whitelist: `https://discord.com`, `https://ptb.discord.com`, `https://canary.discord.com` unless debug flag to relax.
3. Query params: `v`, `encoding`, `client_id`; reject unsupported version (`!=1`) or encoding (anything except json).
4. Wrap send path to serialize JSON; attach metadata (clientId, socketId).
5. Emit READY on connection.
6. Forward incoming JSON to command handler; ensure unknown fields are ignored gracefully.
7. Add health endpoint `/healthz` (JSON `{status:"ok"}`) and metrics endpoint (feature gated, optional `prometheus`).

---

## 5. Bridge Server (Web Clients)
Tasks:
1. Axum route upgrading on configurable port (default 1337 / env `ARRPC_BRIDGE_PORT`).
2. Keep last message per `socketId`; upon new connection replay non-null activities.
3. Broadcast using a shared `broadcast::Receiver<ActivityMessage>`; serialization exactly matches Node messages.
4. Allow any origin by default (since mod scripts run in Discord pages referencing `ws://127.0.0.1:1337`). Optional restriction.
5. Provide CLI flag `--bridge-port` to override (precedence: CLI > env > default).

---

## 6. Process Scanning
Tasks:
1. OS abstraction trait `ProcessBackend` with `async fn list(&self) -> Vec<ProcessInfo>` (pid, exe_path, args?).
2. Linux: replicate `/proc/<pid>/cmdline` logic; robust error handling (skip unreadable). Consider caching exe resolution.
3. Windows: use `CreateToolhelp32Snapshot` (WinAPI) or WMI fallback; parse command line if feasible (optional for MVP) — replicating current wmic CSV approach but replacing with native API for performance.
4. (Future) macOS backend (feature gated, placeholder for now).
5. Matching algorithm:
   - Normalize path: lowercase, forward slashes.
   - Generate tail segments progressively + variant set removing `64`, `.x64`, `_64`, `x64` (mirroring JS heuristics).
   - For each detectable entry executables: skip if `is_launcher`; match name logic (leading `>` indicates arg-based Java detection) — if first char is `>` require raw executable name match and arguments substring match.
6. Maintain maps: timestamps (first seen), names, pids.
7. Every scan (interval 5s): produce SET_ACTIVITY for detected, emit activity null for lost.
8. Concurrency: run scanning in its own task with cancellation token.
9. Provide CLI flag `--no-process-scanning` and env `ARRPC_NO_PROCESS_SCANNING` (same semantics as Node).
10. Unit tests for matcher with synthetic detectables dataset.

---

## 7. Detectables Database Management
Tasks:
1. Determine path: `~/.arrpc/detectables.json` (use `std::env::home_dir()` crate for home; create directory).
2. On startup: if file missing OR `--refresh-detectables` flag OR file older than configurable TTL (default 7 days) → fetch from `https://discord.com/api/v9/applications/detectable`.
3. Use `reqwest` (feature gated: `network`) else fallback to embedded snapshot (generated at build time via `build.rs`).
4. Validate JSON: must parse to array; minimal field validation (id, name, executables optional).
5. Provide log: old count → new count; list new names when debug.
6. Expose `Detectables` handle with `Arc<Vec<Detectable>>` for zero-copy reads.
7. Consider compression (gz) support (optional).
8. On shutdown no rewrite unless updated.

---

## 8. Command Handling Logic
Parity features to implement:
1. `SET_ACTIVITY`:
   - Interpret `activity`, `pid`.
   - Handle clearing (null) and send confirmation with original structure (name:"" when returning, matching Node) and include `application_id` + `type:0`.
   - Translate `buttons` to `metadata.button_urls` + top-level `buttons` labels array.
   - Timestamp normalization (seconds→ms).
   - `instance` flag → bit flag (1<<0) in `flags`.
2. `CONNECTIONS_CALLBACK`: Reply with `{ code: 1000 }`, `evt: ERROR` (matching Node workaround).
3. `INVITE_BROWSER` / `GUILD_TEMPLATE_BROWSER`: expose callback registration; user extension API to plug validation logic; default accept (or reject?) — replicate Node pattern (server emits event; callback determines response). Provide CLI option to auto-accept.
4. `DEEP_LINK`: Accept args struct, allow extension to handle, respond with error on failure.
5. Activity persistence per socket ID; on socket close emit CLEAR.
6. READY event data identical to Node (cdn, api endpoint, environment production, mock user object fields + avatar hash).

---

## 9. Configuration & CLI
CLI (using `clap`):
- Flags: `--no-process-scanning`, `--bridge-port <port>`, `--refresh-detectables`, `--detectables-ttl <hours>`, `--log-format <pretty|json>`, `--config <path>`.
- Environment variable mapping: `ARRPC_NO_PROCESS_SCANNING`, `ARRPC_BRIDGE_PORT`, `ARRPC_DEBUG`.
- Config file (optional) at `~/.arrpc/config.toml` overriding defaults; CLI has highest precedence.

---

## 10. Logging & Observability
Tasks:
1. Use `tracing` + `tracing-subscriber` with env filter (default info; if `ARRPC_DEBUG` then debug).
2. Provide structured JSON logging option.
3. Add span context per connection (socketId) for grouping.
4. Optionally expose metrics (feature `metrics`) with counters: active_connections, activities_set, processes_detected, detectables_count.

---

## 11. Known Issues / Fixes to Preserve
From changelog & code reading:
- [x] Accept blank activities (name may be empty) and still respond (v3.0.0 note).
- [ ] Clear activities on disconnect (implemented; must replicate).
- [x] Proper READY packet config (v3.4.0 rewrite) — copy exact structure.
- [ ] Account connecting bug workaround: `CONNECTIONS_CALLBACK` responding with code 1000 + evt ERROR.
- [ ] Linux scanning improvements (retain reliability; ensure not crashing on unreadable `/proc/<pid>/cmdline`). (placeholder)
- [x] Timestamps conversion logic (already tolerant) — replicate.
- [x] Port scanning for WebSocket RPC (6463–6472) must gracefully continue if EADDRINUSE.
- [ ] Bridge catch-up logic: On connect, send all non-null latest activities by socketId.
- [ ] Heuristic path variant generation for detection (including ignoring 64-bit suffix variants) maintained.

---

## 12. Testing Strategy
Unit Tests:
- [x] Frame codec encode/decode & error paths (invalid op, truncated payload, handshake ordering).
- [ ] Activity normalization (buttons mapping, timestamp conversion, flags calculation).
- [ ] Process matcher (synthetic dataset to ensure variant matching works).
- [ ] Detectables loader (stale vs fresh logic; invalid JSON handling fallback).

Integration Tests (where feasible):
- [ ] Start server with IPC + WS + Bridge (random ports); connect mock client -> perform handshake -> send SET_ACTIVITY -> receive confirmation & bridge broadcast.
- [ ] Process scanning mock backend injecting processes to produce activities then removal triggers clears.

Optional (future CI): property test frame parser (proptest), benchmarking process scanning.

---

## 13. Security & Robustness
- [ ] Validate all client-provided JSON fields; cap payload size (e.g. 64KB) to avoid memory abuse.
- [x] Origin checks on WS RPC; Bridge is local only (bind 127.0.0.1 explicitly).
- [ ] Graceful shutdown (Ctrl+C) triggers broadcast of clears, flush logs.
- [ ] Panic hooks -> structured error log, non-zero exit.
- [ ] Avoid blocking operations in async tasks (use `tokio::fs` / spawn blocking for heavy file IO if needed).

---

## 14. Performance Considerations
- [ ] Pre-allocate serde `String` buffers for frequent frames; consider `simd-json` feature for high-throughput (optional).
- [x] Process scan: diff approach — store previous `HashSet` of matched ids; break early when all executables matched. (initial diff logic)
- [ ] Use `parking_lot` locks or lock-free channels for high frequency events (activities) if contention observed.

---

## 15. Example & Client Updates
Tasks:
- [ ] Port `examples/bridge_mod.js` unchanged (compat layer: JSON wire format identical).
- [ ] Provide updated README usage: `arrpc` binary; environment variables; process scanning toggle.
- [ ] Document how to embed library in Electron-like environments (Rust binary separate; JS mod same).
- [ ] Userscript / extension: update repository links if needed but no protocol changes.

---

## 16. Build, Distribution & Packaging
Tasks:
- [x] Provide `Cargo.toml` workspace and features matrix.
- [ ] Cross-compilation notes (Windows, Linux, macOS) — use GitHub Actions (future) with release artifacts.
- [ ] Optional `--print-socket-paths` CLI to debug IPC discovery.
- [ ] Add `cargo xtask` (optional) for generating embedded detectables snapshot.

---

## 17. Migration Plan (Phased)
Phase 1: Core & Models
1. Define protocol structs + serde (no IO yet).
2. Implement codec + tests.

Phase 2: IPC & WS Servers
3. IPC implementation + handshake + tests.
4. WS RPC server + READY + command dispatch.

Phase 3: Bridge & Event Bus
5. Global event router + bridge broadcast + catch-up logic.

Phase 4: Detectables & Process Scanning
6. Detectables loader + persistence.
7. Linux & Windows backends + matcher.

Phase 5: Command Handling Parity
8. Implement all commands logic & callbacks API.

Phase 6: CLI & Config
9. Add binary crate, flags, logging config.

Phase 7: Testing & Hardening
10. Integration tests, fuzz paths, error injection.

Phase 8: Documentation & Examples
11. Update README, examples, usage notes.

Phase 9: Stretch & Polish
12. Metrics, JSON logging polish, Windows named pipe reliability.

---

## 18. Stretch Goals / Backlog
- [ ] Implement Erlpack / ETF encoding (use existing crates or implement minimal subset) with content negotiation.
- [ ] HTTP transport (REST) mirroring Discord local RPC (for completeness).
- [ ] macOS process scanning backend.
- [ ] Live detectables hot reload API endpoint.
- [ ] Activity filtering rules (privacy / allow list / deny list).
- [ ] Optional TLS wrapper for bridge (likely unnecessary for localhost).
- [ ] Plugin system for custom commands.

---

## 19. Open Questions
- [ ] Should we support multiple simultaneous IPC sockets (like Discord does) or only first free? (Current JS chooses one; keep that for now.)
- [ ] Retain exact mock user ID & avatar? (Yes for compatibility; document rationale.)
- [ ] TTL for detectables default 7 days or 24 hours? (Decide: use 24h for freshness unless offline; configurable.)
- [ ] Provide JSON schema for detectables subset? (Could add for validation later.)

---

## 20. Definition of Done (Parity)
- [ ] Running `arrpc` reproduces behavior: RPC-enabled app connects (IPC or WS) → sets activity → web bridge mod shows status in Discord Web with assets resolving externally.
- [ ] All commands respond identically (structure + success/error semantics) to JS version for supported set.
- [ ] Clearing on disconnect, periodic process scanning updates present, detectables file created at first run in `~/.arrpc/`.
- [ ] Logging clean & structured; debug mode trace-level with frame dumps.
- [ ] Repository includes tasks.md (this file) and updated docs.

---

## 21. Immediate Next Steps
1. [x] Initialize `Cargo.toml` workspace + crates skeleton per Section 1.
2. [x] Implement protocol models + frame codec tests.
3. [ ] Create detectables loader with embedded fallback snapshot (export existing JSON as seed).

---

Feel free to request generation of the initial Rust workspace when ready.
