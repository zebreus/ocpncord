# ocpncord

A `no_std`+`alloc` ocpncord TUI client for the opencode AI coding agent. Connects to a running opencode server over HTTP. Runs on embedded hardware with a TUI display, and natively on desktop for testing.

## Language

**Agent** (the server):
The opencode server — a headless AI coding agent with a REST API on port 4096.
_Avoid_: Backend (overloaded), LLM (too narrow)

**Agent** (the type):
A named configuration on the server that defines a mode (`primary`/`subagent`/`all`), optional model binding, permissions, and a system prompt. Built-in agents include `build` (full tool access) and `plan` (restricted, read-only). The TUI lists agents via `Backend::list_agents()` and cycles through primary agents with Tab. The PromptBar displays the currently active agent name.
_Avoid_: Mode (deprecated term for this concept)

**Backend**:
The Rust trait in the `backend` crate that abstracts the Agent protocol. Implementations include `ocpncord-backend-opencode` (HTTP) and mocks used in tests.
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

**Theme**:
A collection of semantic `Style` values (colours, modifiers) used throughout the TUI. Defined as a plain struct with a `Default` impl (TokyoNight palette). Every `Screen::render()` receives a `&Theme`. Swappable later by loading from `tui.json` config.
_Avoid_: Hardcoding colours in widget code.

**TUI**:
The terminal/embedded user interface built with Ratatui widgets. Platform-agnostic; renders via Crossterm on desktop and mousefood on embedded. Receives generic keypress events (SDL2-style scancodes), not tied to any input hardware.

**Modal**:
An overlay dialog drawn on top of a full-screen view. Used for session list, settings, model picker, and command palette. The underlying screen stays visible underneath (typically dimmed).

**Screen**:
A full-screen view in the TUI. There are exactly two: `StartPage` (shown on launch) and `Chat` (the primary interaction mode). All other views are **Modals** overlaid on the current screen.

**StartPage**:
The initial screen shown on launch. Displays the opencode logo (ASCII art) centered vertically, with the **PromptBar** centered below it (not docked to the bottom). Shows model/agent indicators and a tip line. Typing a message and pressing Enter transitions seamlessly to the **Chat** screen.

**PromptBar**:
The text input widget at the bottom of the **Chat** screen (or centered on the **StartPage**). Accepts plain text messages, `/` slash commands, `!` shell commands, and `@` file references. Shows the current model and active agent (build/plan).

**Chat**:
The primary interaction screen. Shows a scrollable list of **Messages** (user + assistant) in the main area, with the **PromptBar** docked at the bottom. Entered by sending a message from the **StartPage** or selecting a **Session**.

## Relationships

- The **TUI** calls a **Backend** to interact with the **Agent** (the server)
- The **TUI** lists **Agent**s (the type) from the server via `Backend::list_agents()` and passes the selected agent name to `prompt()`/`command()`
- A **Session** contains zero or more **Messages**
- A **Message** contains zero or more **Parts**
- **Prompt** and **Command** both yield a **PromptStream**
- The **EventStream** is a separate out-of-band channel for server-side events
- The **Backend** trait is implemented by `ocpncord-backend-opencode` for the real **Agent**
- The `types` crate holds pure data types and is publishable independently

## Workspace layout

```
types/          — pure data types, serde, no_std+alloc, publishable
backend/        — Backend trait, data types, streaming types, no_std+alloc
ocpncord-backend-opencode/ — HTTP implementation of Backend using reqwless
tui/            — Ratatui widgets, no_std via mousefood, std via Crossterm
native/         — Binary: tokio + Crossterm, the only binary crate
```

## Example dialogue

> **Dev:** "Does the TUI know the Agent is an opencode server?"
> **Domain expert:** "No — the TUI only imports the Backend trait. The fact that it's an opencode server is an implementation detail of `ocpncord-backend-opencode`."

> **Dev:** "Should I use `subscribe()` to get streaming responses?"
> **Domain expert:** "No — streaming responses come from `prompt()`, which returns a `PromptStream`. The `EventStream` from `subscribe()` is for out-of-band events like session creation from another client."
