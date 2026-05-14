# 0001 — Backend trait for protocol abstraction

The client architecture is built around a `Backend` trait in a dedicated `backend` crate. The `tui` crate depends only on this trait, never on concrete implementations like `backend-opencode`. This lets every consumer (widgets, tests, the native binary) program against an interface that can be stubbed, mocked, or swapped without touching the HTTP stack.

The alternative was to let the TUI call `backend-opencode` directly — simpler, one fewer crate, but every integration test would require a real opencode server running. Given the target is `no_std` embedded hardware, running a full server for unit tests is impractical. A mock backend that returns canned data in `no_std` is cheap and fast.
