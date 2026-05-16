# OpenCode Rust Client

`no_std`+`alloc` Rust client for the opencode AI coding agent. See `CONTEXT.md` for domain vocabulary ("Agent" = the server, "Backend" = the Rust trait).

All `no_std` crates use `#![cfg_attr(not(test), no_std)]` + `extern crate alloc;`. Code in `alloc`-using crates must use `alloc::` not `std::`.

## Workspace

| Crate | Dir | `no_std` | Role |
|---|---|---|---|
| `opencode-types` | `types/` | yes | Pure data types, serde only dep, publishable |
| `opencode-backend` | `backend/` | yes (`not(test)`) | `Backend` trait + event/error/mock; re-exports `opencode_types` via `pub use` |
| `opencode-backend-opencode` | `backend-opencode/` | yes (no `std` feature) | HTTP impl via reqwless + embedded-nal-async, generic over TCP transport |
| `opencode-tui` | `tui/` | yes (`not(test)`) | Ratatui widgets, platform-agnostic key events |
| `opencode-native` | `native/` | no | **Single binary** via `[[bin]]` (no `lib.rs`) — tokio + Crossterm |

TUI depends only on `Backend` trait (`opencode-backend`), never on `backend-opencode`. `tui` always enables `mock` feature on `opencode-backend` as a regular dep.

**TUI is under active development** — `.scratch/tui-implementation/PRD.md` is the design source of truth; rendered code may lag. Cross-reference PRD when working in `tui/`.

## Commands

```sh
cargo check                            # only verification step (no CI, no formatter/linter, no rust-toolchain.toml)
cargo build
cargo run -p opencode-native -- --url http://localhost:4096
                                       # requires `opencode serve --port 4096`
cargo test                             # no_std/unit tests only (no server)
cargo test -p opencode-backend-opencode# SSE parsing unit tests in stream.rs
cargo test -p opencode-backend-opencode --test probe -- --nocapture
                                       # raw HTTP probe (needs server, no opencode_types)
cargo test -p opencode-backend-opencode --test integration -- --nocapture
                                       # requires server; prompts call real AI
cargo test --workspace                 # includes integration tests (needs server)
```

## Key facts

- Consumers import from `opencode_backend` (not `opencode_types`); `backend/src/lib.rs` re-exports via `pub use opencode_types::*`.
- `Backend` trait uses `#[allow(async_fn_in_trait)]` — preserve for MSRV.
- All serde types use `#[serde(rename_all = "camelCase")]` — JSON from the Agent API is camelCase.
- Uses `ratatui-core` 0.1 (not the main `ratatui` crate).
- Workspace deps centralized in root `Cargo.toml`; `resolver = "2"` (important for `no_std`).
- `backend-opencode`: no `std` feature — unconditionally `no_std` (except test). Callers supply `embedded-nal-async` impls via `new(transport, dns)`. The native binary provides `StdTcp`/`StdDns` wrappers over tokio.
- `prompt`/`command`/`subscribe` all use `HttpClient::new(transport, dns)` under the hood. The native binary also runs a raw-tokio SSE background task for `/global/event` (separate from the `Backend` trait).
- `MockBackend` (feature `mock` on `opencode-backend`) returns canned data for TUI tests.
- `App<B: Backend>`. Screens: `StartPage`, `Chat`, `Terminal` (PTY). PromptBar modes: `/` commands, `!` shell, `@` fileref, `#` toolref.
- Key bindings (`tui/src/key_chord.rs`): `Ctrl+C` quit, `Ctrl+X` leader (40-tick timeout → Q/L/M/H/T/D/O), `Ctrl+P` palette. `Esc` closes modal / interrupts streaming. `Tab`/`Shift+Tab` cycles agents.
- `tui::Event`/`tui::Scancode` — platform-agnostic input abstraction (SDL2-style). `native/src/main.rs`'s `translate_crossterm_event` maps OS events.
- Native binary accepts `--url` (default `http://localhost:4096`), uses `clap`.

## Agent skills

Issues: local markdown under `.scratch/<feature-slug>/`. Default five-role triage labels. See `docs/agents/` for issue-tracker, domain, and triage-label docs. Installed skills in `skills-lock.json`.
