# OpenCode Rust Client

`no_std`+`alloc` Rust client for [opencode](https://opencode.ai), the AI coding agent.

Connects to a running `opencode serve` instance over HTTP. Targets desktop (tokio + crossterm) and embedded terminals with a TUI display.

## Workspace

| Crate | Role | `no_std` | Publishable |
|---|---|---|---|
| [`opencode-types`](./types/) | Pure data types, serde, JSON contract types | yes | yes |
| [`opencode-backend`](./backend/) | `Backend` trait + streaming types + mock backend | yes | yes |
| [`opencode-backend-opencode`](./backend-opencode/) | HTTP implementation via reqwless over any TCP transport | yes (opt-in `std`) | — |
| [`opencode-tui`](./tui/) | Ratatui widgets, platform-agnostic key events | yes | — |
| [`opencode-native`](./native/) | Binary: tokio + crossterm | no | — |

## Quick start

```sh
# start an opencode server
opencode serve --port 4096

# run the TUI (another terminal)
cargo run -p opencode-native
```

The native binary defaults to `http://localhost:4096`.

## Architecture

The `Backend` trait in `opencode-backend` abstracts the opencode server protocol. The TUI depends only on this trait — it never imports `opencode-backend-opencode` directly. This lets you test the TUI with `MockBackend` (feature `mock`) or swap in a different transport for embedded targets.

```
                    ┌──────────┐
                    │  native  │  (tokio + crossterm, binary only)
                    └────┬─────┘
                         │
              ┌──────────┴──────────┐
              │    opencode-tui     │  (ratatui widgets, no_std)
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
              │  opencode-backend   │  (Backend trait, no_std)
              └──────────┬──────────┘
                    ┌────┴────┐
                    │  HTTP   │   (opencode-backend-opencode)
                    │  Mock   │   (backend mock feature, for tests)
                    └─────────┘
```

## Using the crates

```toml
# pure types (serde, no_std)
opencode-types = { git = "https://github.com/anomalyco/opencode-rust-client" }

# Backend trait + streaming types (futures-core, no_std)
opencode-backend = { git = "..." }

# HTTP backend (reqwless, no_std without "std" feature)
opencode-backend-opencode = { git = "...", default-features = false }
```

## Status

The Backend trait and HTTP implementation are complete (14/14 integration tests passing). The TUI is under active development — see [`.scratch/tui-implementation/PRD.md`](./.scratch/tui-implementation/PRD.md).

## License

MIT
