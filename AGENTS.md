# OpenCode Rust Client

`no_std`+`alloc` Rust client for the opencode AI coding agent. See `CONTEXT.md` for domain vocabulary ("Agent" = the server, "Backend" = the Rust trait).

## Workspace

| Crate | Dir | `no_std` | Role |
|---|---|---|---|
| `opencode-types` | `types/` | yes | Pure data types, serde, publishable |
| `opencode-backend` | `backend/` | yes (`not(test)`) | `Backend` trait + event/error/mock; re-exports `opencode_types` via `pub use` |
| `opencode-backend-opencode` | `backend-opencode/` | yes (opt-in `std` feature) | HTTP impl via reqwless, generic over TCP transport |
| `opencode-tui` | `tui/` | yes (`not(test)`) | Ratatui widgets, platform-agnostic key events |
| `opencode-native` | `native/` | no | **Single binary** via `[[bin]]` (no `lib.rs`) — tokio + Crossterm |

TUI depends only on `Backend` trait (`opencode-backend`), never on `backend-opencode` (see `docs/adr/0001-backend-trait-for-protocol-abstraction.md`). `tui` always enables `mock` feature on `opencode-backend` as a regular dep.

## Commands

```sh
cargo check                            # basic verification (no CI, no formatter/linter config)
cargo build
cargo run -p opencode-native           # requires `opencode serve --port 4096` running
cargo test                             # no_std/unit tests only (no server needed)
cargo test -p opencode-backend-opencode# unit tests: SSE parsing in backend-opencode/src/stream.rs
cargo test --workspace                 # includes integration tests (needs server)
cargo test -p opencode-backend-opencode --test integration -- --nocapture
                                       # requires `opencode serve --port 4096`
```

No CI, `rust-toolchain.toml`, formatter, or linter config. `cargo check` is the only verification step.

## Key facts

- `Backend` trait uses `#[allow(async_fn_in_trait)]` in `backend/src/lib.rs` — preserve for MSRV.
- `backend/src/lib.rs` re-exports all of `opencode_types: pub use opencode_types::*`. Consumers should import from `opencode_backend`, not `opencode_types`.
- `backend-opencode`: `default = ["std"]` (tokio transport). Without `std` feature, callers supply `embedded-nal-async` impls via `new(transport, dns)`. Use `new_std()` for native.
- All serde types use `#[serde(rename_all = "camelCase")]` — JSON from the Agent API is camelCase.
- Uses `ratatui-core` 0.1 (not the main `ratatui` crate) — avoid adding the wrong ratatui dep.
- Workspace uses `resolver = "2"` — important for `no_std` crate resolution with optional deps.
- Workspace dependencies centralized in root `Cargo.toml` (`serde`, `futures-core`, `ratatui-core`, etc.) — add new deps there if shared.
- `BufferedStream`/`parse_sse` (in `backend-opencode/src/stream.rs`) parses all SSE events upfront for `prompt`/`command`; `subscribe` uses real-time `TcpSseStream` on std.
- `MockBackend` (feature `mock` on `opencode-backend`) returns canned data for TUI tests without a server.
- `tui::Event`/`tui::Scancode` — platform-agnostic input abstraction (SDL2-style scancodes). Platform layer (`native/src/main.rs`'s `translate_crossterm_event`) translates OS input.
- `App` is generic over `B: Backend`. Screens: `ScreenId::StartPage` (logo + tip) and `ScreenId::Chat` ("No messages yet" placeholder). `PromptBar` supports `/` commands / `!` shell / `@` fileref modes. `KeyChord` leader-key system.
- Key bindings: `Ctrl+C` quits. `Ctrl+X` enters leader mode (40-tick timeout), then `q` quits. Typing `/new` + Enter on empty session resets to StartPage.
- `tui` uses `ratatui-core::backend::TestBackend` in tests (std-only, works because `#[cfg_attr(not(test), no_std)]`).
- Integration tests (`backend-opencode/tests/integration.rs`) call a real server — expensive, not for casual runs.
- TUI under active development: see `.scratch/tui-implementation/PRD.md`.

## Agent skills

Issues: local markdown under `.scratch/<feature-slug>/`. Default five-role triage labels. See `docs/agents/` for issue-tracker, domain, and triage-label docs. Installed skills in `skills-lock.json`.
