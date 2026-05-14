# OpenCode Rust Client

A `no_std`+`alloc` Rust client for the opencode AI coding agent. Connects to a running opencode server over HTTP. Runs on embedded hardware with a TUI display, and natively on desktop for testing.

## Language

**Agent**:
The opencode server — a headless AI coding agent with a REST API on port 4096.
_Avoid_: Backend (overloaded), LLM (too narrow)

**Backend**:
The Rust trait in the `backend` crate that abstracts the Agent protocol. Implementations include `backend-opencode` (HTTP) and mocks used in tests.
_Avoid_: Client, transport

**BackendEvent**:
A single item yielded by a `PromptStream` or `EventStream`. Covers text deltas, tool state changes, step markers, errors, and stream completion.

**EventStream**:
An async `Stream` of `BackendEvent`s from the SSE `/events` subscription. Carries out-of-band notifications (session created/deleted, status changes).
_Avoid_: Confusing with `PromptStream` — they are separate streams

**Message**:
A single turn in a **Session**, authored by either the **User** or the **Assistant** (Agent). Contains zero or more **Part**s.

**Part**:
A typed content chunk within a **Message**. Variants: `Text`, `Tool` (with state: pending/running/completed/error), `StepStart`, `StepFinish`.

**Prompt**:
Sending a user message to the **Agent** and receiving a streaming response. The primary interaction mode.
_Avoid_: Command (refers to a specific API call)

**Command**:
An API call to the Agent that expects a non-streaming structured response (e.g., "run a shell command"). Returns the same `PromptStream` event types.

**PromptStream**:
An async `Stream` of `BackendEvent`s returned by `prompt()` or `command()`. Carries the Agent's response as it is generated.

**Session**:
A single conversation with the **Agent**, consisting of a sequence of **Message**s.

**TUI**:
The terminal/embedded user interface built with Ratatui widgets. Platform-agnostic; renders via Crossterm on desktop and mousefood on embedded. Receives generic keypress events (SDL2-style scancodes), not tied to any input hardware.

## Relationships

- The **TUI** calls a **Backend** to interact with the **Agent**
- A **Session** contains zero or more **Messages**
- A **Message** contains zero or more **Parts**
- **Prompt** and **Command** both yield a **PromptStream**
- The **EventStream** is a separate out-of-band channel for server-side events
- The **Backend** trait is implemented by `backend-opencode` for the real **Agent**
- The `types` crate holds pure data types and is publishable independently

## Workspace layout

```
types/          — pure data types, serde, no_std+alloc, publishable
backend/        — Backend trait, data types, streaming types, no_std+alloc
backend-opencode/ — HTTP implementation of Backend using reqwless
tui/            — Ratatui widgets, no_std via mousefood, std via Crossterm
native/         — Binary: tokio + Crossterm, the only binary crate
```

## Example dialogue

> **Dev:** "Does the TUI know the Agent is an opencode server?"
> **Domain expert:** "No — the TUI only imports the Backend trait. The fact that it's an opencode server is an implementation detail of `backend-opencode`."

> **Dev:** "Should I use `subscribe()` to get streaming responses?"
> **Domain expert:** "No — streaming responses come from `prompt()`, which returns a `PromptStream`. The `EventStream` from `subscribe()` is for out-of-band events like session creation from another client."
