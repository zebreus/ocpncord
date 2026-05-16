# OpenCode Rust Client

`no_std`+`alloc` Rust client for the opencode AI coding agent. `CONTEXT.md` defines domain vocabulary ("Agent" = server, "Backend" = Rust trait).

All `no_std` crates use `#![cfg_attr(not(test), no_std)]` + `extern crate alloc;`. Code uses `alloc::` not `std::`.

## Workspace

| Crate | Dir | `no_std` | Role |
|---|---|---|---|
| `opencode-types` | `types/` | yes | Pure data types, serde only dep, publishable |
| `opencode-backend` | `backend/` | yes (`not(test)`) | `Backend` trait + event/error/mock; re-exports `opencode_types` via `pub use` |
| `opencode-backend-opencode` | `backend-opencode/` | yes | HTTP impl via reqwless + embedded-nal-async, generic over TCP transport |
| `opencode-tui` | `tui/` | yes (`not(test)`) | Ratatui widgets, platform-agnostic key events |
| `opencode-native` | `native/` | no | **Single binary** via `[[bin]]` (no `lib.rs`) — tokio + Crossterm |

TUI depends only on `Backend` trait (`opencode-backend`), never on `backend-opencode`. `tui` always enables `mock` feature on `opencode-backend` as a regular dep.

`backend-opencode` is unconditionally `no_std` (no `std` feature gate). Callers supply `embedded-nal-async` impls via `new(transport, dns)`. The native binary provides `StdTcp`/`StdDns` wrappers over tokio.

**TUI under active development** — `.scratch/tui-implementation/PRD.md` is design source of truth; cross-reference when working in `tui/`.

## Commands

```sh
cargo check                            # only verification (no CI, no formatter/linter, no rust-toolchain.toml)
cargo build
cargo run -p opencode-native -- --url http://localhost:4096
                                       # requires `opencode serve --port 4096`
cargo test --lib                       # unit tests only, no server needed
cargo test -p opencode-backend-opencode --test integration -- --nocapture
                                       # 14 tests, requires server
```

`cargo test` (without `--lib`) compiles all tests including integration. Use `--lib` when no server is running.

## Key facts

- All serde types use `#[serde(rename_all = "camelCase")]` — JSON from the Agent API is camelCase.
- `prompt()`/`command()` are fire-and-forget (HTTP returns immediately, response through SSE). `subscribe()`/`sync_events()` use `BufferedStream` for SSE parsing.
- `MockBackend` (feature `mock` on `opencode-backend`) returns canned data for TUI tests.
- `App<B: Backend>`. Screens: `StartPage`, `Chat`, `Terminal` (PTY). PromptBar modes: `/` command, `!` shell, `@` fileref.
- Key bindings (`tui/src/key_chord.rs`): `Ctrl+C` quit, `Ctrl+X` leader (40-tick timeout → Q/L/M/H/T/D/O), `Ctrl+P` palette, `D`/`O` both toggle side panel. `Esc` closes modal / interrupts streaming. `Tab`/`Shift+Tab` cycles agents.
- Workspace deps in root `Cargo.toml`, `resolver = "2"`. Uses `ratatui-core` 0.1 (not main `ratatui`).
- Native binary accepts `--url` (default `http://localhost:4096`), uses `clap`.

## Agent skills

Issues: local markdown under `.scratch/<feature-slug>/`. Default five-role triage labels. See `docs/agents/` for issue-tracker, domain, and triage-label docs. Installed skills in `skills-lock.json`.
