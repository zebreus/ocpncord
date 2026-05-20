# ocpncord

`no_std`+`alloc` ocpncord TUI client for [opencode](https://opencode.ai), the AI coding agent.

Connects to a running `opencode serve` instance over HTTP. Targets desktop (tokio + crossterm) and embedded terminals with a TUI display.

## Workspace

| Crate | Role | `no_std` | Publishable |
|---|---|---|---|
| [`ocpncord-types`](./types/) | Pure data types, serde, JSON contract types | yes | yes |
| [`ocpncord-backend`](./backend/) | `Backend` trait + streaming types + mock backend | yes | yes |
| [`ocpncord-backend-opencode`](./backend-opencode/) | HTTP implementation via reqwless over any TCP transport | yes | yes |
| [`ocpncord-tui`](./tui/) | Ratatui widgets, platform-agnostic key events | yes | yes |
| [`ocpncord-native`](./native/) | Binary: tokio + crossterm | no | yes |

## Quick start

```sh
# start an opencode server
opencode serve --port 4096

# run the TUI (another terminal)
cargo run -p ocpncord-native
```

The native binary defaults to `http://localhost:4096`.

## Architecture

The `Backend` trait in `ocpncord-backend` abstracts the opencode server protocol. The TUI depends only on this trait — it never imports `ocpncord-backend-opencode` directly. This lets you test the TUI with `MockBackend` (feature `mock`) or swap in a different transport for embedded targets.

```
                    ┌──────────┐
                    │  native  │  (tokio + crossterm, binary only)
                    └────┬─────┘
                         │
              ┌──────────┴──────────┐
               │    ocpncord-tui     │  (ratatui widgets, no_std)
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
               │  ocpncord-backend   │  (Backend trait, no_std)
              └──────────┬──────────┘
                    ┌────┴────┐
                    │  HTTP   │   (ocpncord-backend-opencode)
                    │  Mock   │   (backend mock feature, for tests)
                    └─────────┘
```

## Using the crates

```toml
# pure types (serde, no_std)
ocpncord-types = "0.1"

# Backend trait + streaming types (futures-core, no_std)
ocpncord-backend = "0.1"

# HTTP backend (reqwless, no_std)
ocpncord-backend-opencode = "0.1"
```

## Status

The Backend trait and HTTP implementation are complete (14/14 integration tests passing). The TUI is under active development — see [`.scratch/tui-implementation/PRD.md`](./.scratch/tui-implementation/PRD.md).

## License

AGPL-3.0-only
