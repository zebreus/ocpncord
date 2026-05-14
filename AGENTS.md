# OpenCode Rust Client

`no_std`+`alloc` Rust client for the opencode AI coding agent. `CONTEXT.md` is the domain glossary — use its vocabulary (especially "Agent" = the server, "Backend" = the Rust trait).

## Workspace

| Crate | Role | `no_std` | Key deps |
|---|---|---|---|
| `types/` | Pure data types, serde, publishable | yes | serde |
| `backend/` | `Backend` trait + event/error/mock | yes | futures-core; re-exports `opencode_types` via `pub use` |
| `backend-opencode/` | HTTP impl via reqwless, generic over TCP transport | yes (opt-in via `std` feature) | reqwless, embedded-io-async, embedded-nal-async |
| `tui/` | Ratatui widgets, platform-agnostic key events | yes | ratatui-core 0.1 |
| `native/` | Binary: tokio + Crossterm (only binary crate) | no | tokio |

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
- `MockBackend` (feature `mock` on `opencode-backend`) returns canned data for TUI tests without a server.
- `BufferedStream`/`parse_sse` — parses all SSE events upfront for `prompt`/`command`; `subscribe` uses real-time `TcpSseStream` on std.
- `tui::Event`/`tui::Scancode` — platform-agnostic input abstraction (SDL2-style scancodes). Platform layer translates OS input into these.
- All serde types use `#[serde(rename_all = "camelCase")]` — JSON from the Agent API is camelCase.
- ratatui-core 0.1 (not the main ratatui crate) — avoid adding old ratatui deps.
- Integration tests (`backend-opencode/tests/integration.rs`) call a real server — expensive, not for casual runs.
- `native/src/main.rs` creates `App` + prints; no event loop wired. Default server URL: `http://localhost:4096`.
- `App` holds a `Screen` enum (`SessionList`, `Chat`) — no widget implementations yet.

## Agent skills (mattpocock/skills)

Issues: local markdown under `.scratch/<feature-slug>/`. Default five-role triage labels. See `docs/agents/` for issue-tracker, domain, and triage-label docs. Installed skills in `skills-lock.json`.
