# OpenCode Rust Client

`no_std`+`alloc` Rust client for the opencode AI coding agent. `CONTEXT.md` is the domain glossary — use its vocabulary (especially "Agent" = the server, "Backend" = the Rust trait).

## Workspace

| Crate (dir) | Role | `no_std` | Key deps |
|---|---|---|---|
| `opencode-types` (`types/`) | Pure data types, serde, publishable | yes | serde |
| `opencode-backend` (`backend/`) | `Backend` trait + event/error/mock | yes (`#[cfg_attr(not(test), no_std)]`) | futures-core; re-exports `opencode_types` via `pub use` |
| `opencode-backend-opencode` (`backend-opencode/`) | HTTP impl via reqwless, generic over TCP transport | yes (opt-in via `std` feature) | reqwless, embedded-io-async, embedded-nal-async |
| `opencode-tui` (`tui/`) | Ratatui widgets, platform-agnostic key events | yes (`#[cfg_attr(not(test), no_std)]`) | ratatui-core 0.1; depends on `opencode-backend` with `mock` feature |
| `opencode-native` (`native/`) | Binary: tokio + Crossterm (only binary crate) | no | tokio, crossterm, ratatui-core (std) |

TUI depends only on `Backend` trait in `backend`, never on `backend-opencode` (see `docs/adr/0001-backend-trait-for-protocol-abstraction.md`).

## Commands

```sh
cargo check
cargo build
cargo test --workspace                              # includes integration tests (needs server)
cargo test -p opencode-backend-opencode             # unit tests only (SSE parsing in stream.rs)
cargo test -p opencode-backend-opencode --test integration -- --nocapture
                                                    # requires `opencode serve --port 4096`
```

No CI, `rust-toolchain.toml`, formatter, or linter config — `cargo check` is the basic verification step.

## Notable details

- `Backend` trait uses `#[allow(async_fn_in_trait)]` in `backend/src/lib.rs` — preserve for MSRV.
- `backend/src/lib.rs` re-exports all of `opencode_types`: `pub use opencode_types::*`. Consumers rarely import `types` directly.
- `backend-opencode`: `default = ["std"]` (tokio transport). Without `std`, callers supply `embedded-nal-async` impls. Use `new_std()` for native; `new(transport, dns)` for embedded.
- `MockBackend` (feature `mock` on `opencode-backend`) returns canned data for TUI tests without a server. `tui` always enables `mock` as a regular (not dev) dependency.
- `BufferedStream`/`parse_sse` — parses all SSE events upfront for `prompt`/`command`; `subscribe` uses real-time `TcpSseStream` on std.
- `tui::Event`/`tui::Scancode` — platform-agnostic input abstraction (SDL2-style scancodes). Platform layer translates OS input into these.
- All serde types use `#[serde(rename_all = "camelCase")]` — JSON from the Agent API is camelCase.
- ratatui-core 0.1 (not the main ratatui crate) — avoid adding old ratatui deps.
- Integration tests (`backend-opencode/tests/integration.rs`) call a real server — expensive, not for casual runs.
- `native/src/main.rs` has a full event loop (Crossterm, 50ms tick via `tokio::time::interval`, alternate screen). Default server URL: `http://localhost:4096`.
- `App` is generic over `B: Backend`. Screens: `ScreenId::StartPage` (logo + tip) and `ScreenId::Chat` ("No messages yet" placeholder). Widgets exist: `StartPage`, `Chat`, `PromptBar` (text input with `/` command / `!` shell / `@` fileref modes), `KeyChord` (leader-key system).
- Key bindings: `Ctrl+C` quits immediately. `Ctrl+X` enters leader mode (40-tick timeout), then `q` quits. Typing `/new` + Enter on empty session resets to StartPage.
- `tui` uses `ratatui-core::backend::TestBackend` in tests (std-only, works because `#[cfg_attr(not(test), no_std)]`).
- Workspace uses `resolver = "2"` — important for `no_std` crate resolution with optional deps.

## Agent skills (mattpocock/skills)

Issues: local markdown under `.scratch/<feature-slug>/`. Default five-role triage labels. See `docs/agents/` for issue-tracker, domain, and triage-label docs. Installed skills in `skills-lock.json`.
