# drpc

Experimental Discord Local RPC compatible server in Rust.

## Crates

- `drpc-core` – shared protocol models, frame codec, event bus, detectables loader.
- `drpc-ipc` – Discord IPC (Unix + Windows named pipe) transport implementation.
- `drpc-ws` – WebSocket transport (scans 6463-6472, origin validation, payload caps).
- `drpc-bridge` – Bridge fan-out server for local observers (activity replay on connect).
- `drpc-process` – Process scanning + matching against detectables list.
- `drpc` – Binary wiring everything together (CLI + config).

## Features Implemented

See `tasks.md` for the full milestone roadmap and current status.

## Building

### Prerequisites

- Rust toolchain (1.85+ recommended; project tested with stable).
- Git, a C toolchain like `gcc` or `clang` for tls.

### Linux

```bash
git clone <repo-url> drpc
cd drpc
cargo build --workspace --all-targets
```

Run tests:

```bash
cargo test --all
```

### macOS

Install Rust (via rustup). Then:

```bash
cargo build --workspace
```

### Windows

Install Rust using rustup (MSVC toolchain). Build normally:

```powershell
git clone <repo-url> drpc
cd drpc
cargo build --workspace
```

If you have multiple toolchains installed, explicitly select stable:

```powershell
rustup default stable
```

### Cross Compilation Notes

- IPC transport uses platform specific code paths for Unix domain sockets vs Windows named pipes; ensure the target OS matches your desired runtime.
- No additional native dependencies are required beyond standard Rust targets.

## Running

From the workspace root:

```bash
cargo run -p drpc -- --help
```

Common flags:

- `--bridge-port <port>` – Port for bridge server (default auto).
- `--no-process-scanning` – Disable process scanning subsystem.
- `--refresh-detectables` – Force refresh of detectables file.
- `--detectables-ttl <seconds>` – Override detectables cache TTL.
- `--log-format {pretty|json}` – Select logging output format.

Environment:

- `DRPC_NO_PROCESS_SCANNING=1` – Disable scanning.
- `DRPC_DEBUG=1` – Enable verbose frame-level logging.

## Detectables File

Stored at `~/.drpc/detectables.json` with TTL and refresh logic (see `drpc-core`). Fallback minimal set is embedded for resilience.

## Status

This project is experimental and not affiliated with Discord. Roadmap items and parity goals tracked in `tasks.md`.

## License

MIT.
