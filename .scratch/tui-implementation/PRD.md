Status: finished
Feature: TUI Implementation
Created: 2026-05-14

# PRD: OpenCode Rust Client TUI

## Problem Statement

ocpncord has a working Backend trait, a fully functional HTTP backend implementation (14/14 integration tests passing), and a platform-agnostic types crate. At the start of this feature, the TUI crate was only a stub — an `App` struct with a shallow full-screen-view seam and platform-agnostic `Event` types. The native binary printed "starting..." and exited.

Without a TUI, the client is unusable. Users cannot see sessions, send prompts, view streaming responses, or interact with the agent in any way. The server protocol works; the client is missing the last mile.

## Solution

Build a full terminal UI matching the look, feel, and interaction model of the official opencode TUI (Go/Bubble Tea). The TUI connects to a remote `opencode serve` instance via the existing Backend trait. It runs on desktop (tokio + crossterm) today and can target embedded terminals via the existing `no_std`+`alloc` foundation.

The architecture uses app-owned modes for the full-screen surfaces — **StartPage** (launch), **Chat** (primary interaction), and **Terminal** — with all other views as overlay **Modals** (sessions list, help, model picker, command palette). The **PromptBar** input widget supports live detection of `/` slash commands, `!` shell commands, and `@` file references. Agent switching via Tab cycles through primary agents listed from the server, with the active agent name shown in the PromptBar indicator. Prompt and command submissions return receipts; streaming responses render from the live `/global/event` feed as Parts arrive.

## User Stories

### Launch

1. As a user, I want to run the binary and immediately see a landing screen with the opencode ASCII logo centered, so that I know the application started successfully.
2. As a user, I want to see the PromptBar centered below the logo on the StartPage, so that I can begin interacting immediately.
3. As a user, I want to see the current model and active agent name displayed in the PromptBar, so that I know which configuration is active.
4. As a user, I want to see a tip line displayed below the PromptBar on the StartPage, so that I have guidance on available commands.

### Messaging

5. As a user, I want to type a message in the PromptBar and press Enter to send it, so that I can communicate with the agent.
6. As a user, I want the UI to transition seamlessly from StartPage to Chat screen when I send a message, so that I experience a single fluid interaction.
7. As a user, I want to see my sent message appear immediately in the Chat message list, so that I have confirmation my input was received.
8. As a user, I want to see the agent's response stream in real-time as Parts arrive, so that I can read the response as it is generated.
9. As a user, I want Text parts displayed as readable paragraphs, so that I can read the agent's natural language output.
10. As a user, I want Reasoning parts displayed in italic/yellow with optional toggle visibility, so that I can see the agent's thinking process when desired.
11. As a user, I want Tool parts displayed with their state (pending/running/completed/error), so that I can track what tools the agent is executing.
12. As a user, I want StepStart/StepFinish parts displayed as dividers with snapshot summaries, so that I can follow the agent's workflow progression.
13. As a user, I want the message list to auto-scroll as new content arrives, so that I always see the latest output.
14. As a user, I want to scroll back through the message history manually, so that I can review earlier content.

### Streaming

15. As a user, when the agent is generating a response, I want the PromptBar to show a visual indicator that a response is in progress, so that I know the system is working.
16. As a user, I want to press Escape to interrupt an in-progress streaming response, so that I can stop the agent mid-generation.
17. As a user, I want parts within the currently-streaming message to update in-place as their state changes, so that I see tool state transitions live.

### Agents

18. As a user, I want to press Tab to cycle through available primary agents, so that I can switch between Build and Plan (or other custom agents).
19. As a user, I want the active agent name and its indicator colour (Build=green, Plan=yellow) shown in the PromptBar, so that I know which agent will respond.
20. As a user, I want the active agent selection sent with each prompt, so that the correct agent handles my request.
21. As a user, I want the agent list fetched from the server on start, so that the TUI reflects the server's actual configuration.

### Session Management

22. As a user, I want a session created automatically when I send my first message, so that I don't need to manually create and select sessions.
23. As a user, I want to open the session list modal via `/sessions` or `Ctrl+X L`, so that I can see all my sessions.
24. As a user, I want to select a session from the list to load its messages into the Chat screen, so that I can resume previous conversations.
25. As a user, I want to start a new session via `/new` or `Ctrl+X N`, so that I can begin a fresh conversation.
26. As a user, I want to delete a session from the session list, so that I can clean up unwanted conversations.

### Slash Commands

27. As a user, I want to type `/help` to open the help modal, so that I can see available commands and keybindings.
28. As a user, I want to type `/sessions` to open the session list modal, so that I can switch sessions.
29. As a user, I want to type `/new` to start a new session, so that I can begin a fresh conversation.
30. As a user, I want to type `/exit` to quit the application, so that I can exit gracefully.
31. As a user, I want to type `/models` to open the model picker modal, so that I can switch models.
32. As a user, I want to type `/diagnostics` to open the diagnostics panel, so that I can inspect current errors without leaving the TUI.

### Keybindings

33. As a user, I want to press `Ctrl+X` as a leader key followed by a second key to trigger commands, so that I can navigate efficiently without slash commands.
34. As a user, I want `Ctrl+X H` to open help, so that I can get help quickly.
35. As a user, I want `Ctrl+X Q` to quit, so that I can exit efficiently.
36. As a user, I want `Ctrl+X N` to start a new session, so that I can switch sessions efficiently.
37. As a user, I want `Ctrl+X L` to open the session list, so that I can browse sessions efficiently.
38. As a user, I want `Ctrl+P` to open the command palette, so that I can search and execute commands.
39. As a user, I want Escape to close the current modal, so that I can dismiss dialogs quickly.
40. As a user, I want Escape to interrupt a streaming response when no modal is open, so that I can stop the agent.

### Modals

41. As a user, I want the session list modal to show all sessions with their titles, so that I can choose a session to resume.
42. As a user, I want the help modal to list available slash commands and keybindings, so that I can learn how to use the TUI.
43. As a user, I want the model picker modal to list available models from the server, so that I can switch the active model.
44. As a user, I want the command palette modal to show searchable commands, so that I can discover and execute any action.
45. As a user, I want modals to dim the background screen, so that I can still see context behind the dialog.
46. As a user, I want modals to close when I press Escape or click outside the modal area, so that I can dismiss them naturally.

### Input

47. As a user, I want the PromptBar to visually change appearance when I type `/`, `!`, or `@` at the start of input, so that I know what mode the input is in.
48. As a user, I want to type `@` followed by a filename to reference a file in my prompt, so that the agent knows which file I mean.
49. As a user, I want to type `!` followed by a shell command and press Enter to execute it inline, so that I can run commands without leaving the TUI.

### Theme

50. As a user, I want the TUI to use a pleasant dark colour scheme (TokyoNight-inspired) by default, so that it is comfortable to use in a terminal.
51. As a user, I want the UI to be visually consistent — all screens and modals using the same semantic colour palette, so that the interface is cohesive.

### General

52. As a user, I want error messages displayed in red when a Backend call fails, so that I know something went wrong.
53. As a user, I want the application to handle disconnection gracefully (show an error state), so that I am not left with a frozen screen.
54. As a user, I want the session title displayed in the Chat screen header, so that I know which session I am in.

## Implementation Decisions

### Architecture: App Modes vs Modals

Full-screen surfaces are app-owned **modes**, not separate Screen adapters. The current modes are **StartPage** (shown on launch), **Chat** (primary interaction), and **Terminal**. All other views are overlay **Modals** drawn on top of the current mode. The base mode renders first, then any active modal is rendered on top with a dimmed overlay background using `ratatui_core::widgets::Clear`.

`App` owns the top-level mode render match, mode-local layout, PromptBar placement, status line, and mode transitions. StartPage rendering is inlined in `App`; `chat.rs` remains a focused transcript-rendering module for Chat message content. The `Modal` trait handles overlay dialogs, and `App` holds an `Option<Box<dyn Modal>>` for the active modal.

### State Management

`App<B: Backend>` holds:

- **active_mode**: enum `StartPage | Chat | Terminal`
- **active_modal**: `Option<Box<dyn Modal>>`
- **active_session**: `Option<Session>`
- **messages**: `Vec<LoadedMessage>` — local copy of message+parts for the active session
- **live_events**: `Option<B::EventStream>` — global live event stream from `Backend::subscribe_live()`, if connected
- **sync_known_sequences**: cursor map used for `/sync/history` catch-up after reconnects
- **partial_parts**: `Vec<Part>` — parts being accumulated for the last (streaming) message
- **agents**: `Vec<Agent>` — cached from `Backend::list_agents()`
- **active_agent**: `usize` — index into agents (the current primary agent)
- **theme**: `Theme`
- **key_chord**: `Option<Scancode>` — leader key state
- **key_chord_tick**: `u64` — tick count when leader was pressed
- **config**: `Config` — cached from `Backend::get_config()`

### Event Loop

The native binary uses `tokio::select!` to multiplex three sources:

```
loop {
    tokio::select! {
        input = read_crossterm_event()  -> Event::Key(translate(input)),
        ev = live_events.next()         -> Event::Backend(envelope.event),
        _ = tick(50ms)                  -> Event::Tick,
    }
    app.handle_event(event);
    terminal.draw(|f| app.render(f))?;
}
```

The `tui` crate receives `Event::Key`, `Event::Backend`, `Event::Tick` through a single `handle_event` method. The driver polls `Backend::EventStream` directly, filters `EventEnvelope` scope metadata, updates cursor state, and replays `sync_history()` batches on reconnect.

The `Event` enum in `tui` gains:

```rust
pub enum Event {
    Key(KeyEvent),
    Backend(BackendEvent),
    Tick,
    Quit,
}
```

### Keybinding System

A `KeyChord` state machine in `tui` handles leader key composition. Direct bindings (`Ctrl+P`, `Tab`, `Escape`) fire immediately. `Ctrl+X` starts leader mode — the next non-modifier keypress completes the chord. A 2-second timeout on the Tick event cancels the leader state.

```rust
pub struct KeyChord {
    leader: Option<Scancode>,
    leader_tick: u64,
}

impl KeyChord {
    pub fn handle(&mut self, key: KeyEvent, tick: u64) -> Option<Action>;
    pub fn tick(&mut self, tick: u64) -> Option<Action>;  // timeout
}
```

### PromptBar / Live Input Detection

The PromptBar widget detects input prefixes in real-time (not just on submit). Based on the first non-whitespace character:

- `/` → command mode (change PromptBar appearance, show command hint)
- `!` → shell mode (change PromptBar appearance)
- `@` → file reference (current behaviour, appearance change)
- none → normal text mode

The detection is character-level and updates the PromptBar's visual style accordingly. The PromptBar also displays the active agent name and current model name on the right side.

### Streaming Rendering

`Backend::submit_prompt()`/`submit_command()` return immediate receipts. Live output arrives through `Backend::subscribe_live()` as `EventEnvelope`s from `/global/event`. `MessagePartUpdated`/`MessagePartDelta` update `App.partial_parts`; assistant `MessageUpdated` finalizes the partial parts into a `LoadedMessage`. `MessageRemoved` and `MessagePartRemoved` remove visible or partial transcript state where possible.

The Chat mode render path in `App` reads `App.messages` (complete) and passes `App.partial_parts` into `chat.rs::render_chat()` if a stream is active. This keeps top-level mode layout in `App` while keeping transcript rendering encapsulated in `chat.rs`, and it ensures in-place updates to tool state transitions.

### Agent Selection

The agent is selected via `Backend::list_agents()` on launch. Only `mode: "primary"` agents are shown for Tab cycling. The selected agent name is passed as the `agent` parameter to `Backend::submit_prompt()` and `Backend::submit_command()`. The active agent is displayed in the PromptBar.

### Session Auto-Creation

No mandatory session picker step. When the user sends the first message and `active_session` is `None`, `App` calls `Backend::create_session()` automatically before calling `submit_prompt()`/`submit_command()`. The created session ID is stored and used for subsequent messages.

### Theme System

The `Theme` is a plain struct with named `Style` fields covering all UI surfaces. A `Default` impl provides a TokyoNight-inspired dark palette. App-owned render helpers and modals receive `&Theme`. Later, themes can be loaded from `tui.json` — the struct shape already supports it.

### Modals

Four modals for MVP:

| Modal           | Trigger                 | Content                                     |
| --------------- | ----------------------- | ------------------------------------------- |
| Session list    | `/sessions`, `Ctrl+X L` | Scrollable list of sessions + select/delete |
| Help            | `/help`, `Ctrl+X H`     | Command reference + keybindings             |
| Model picker    | `/models`, `Ctrl+X M`   | List of models from server                  |
| Command palette | `Ctrl+P`                | Searchable command list                     |

Modals render as a centred rectangle over a dimmed terminal. The `Modal` trait:

```rust
pub trait Modal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect);
    fn handle_event(&mut self, event: Event) -> Action;
    fn title(&self) -> &str;
}
```

`App::render()` always renders the active mode first, then if `active_modal` is `Some`, clears a centred area and renders the modal.

### PartRenderer Module

A deep module that converts `Part` variants into styled ratatui `Line`s. This is pure logic — no IO, no state beyond the Part itself. Interface:

```rust
pub fn render_part(part: &Part, theme: &Theme, show_details: bool) -> Vec<Line>;
```

- `Text(TextPart)` → a `Line` with `theme.part_text`
- `Reasoning(ReasoningPart)` → a `Line` with `theme.part_reasoning` (italic, yellow)
- `Tool(ToolPart)` → depends on state: idle/dim, running/blue with spinner, done/green with checkmark, error/red with cross
- `StepStart(StepStartPart)` → a dim divider line with optional snapshot text
- `StepFinish(StepFinishPart)` → a dim divider line with optional reason

### Action Enum

The single `Action` enum used everywhere:

```rust
pub enum Action {
    None,
    Quit,
    OpenModal(ModalId),
    CloseModal,
    CycleAgent,
    ExecuteCommand(String),
    Interrupt,
    SendMessage,
    OpenPalette,
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
}
```

Slash input, command palette entries, key-chord commands, and `BackendEvent::TuiCommandExecute` all route through a single app-owned TUI command dispatcher. Typed slash commands clear the PromptBar where appropriate; action-triggered commands preserve drafts and use toggle semantics for panels.

## Testing Decisions

### Principles

- Test external behaviour, not implementation details. Do not assert on internal field values of widgets. Assert on what the user sees (rendered output) or what the app does (Actions returned).
- The `MockBackend` (feature `mock` on `ocpncord-backend`) provides canned data. Use it for all TUI tests to avoid server dependency.
- Render tests should snapshot rendered ratatui `Buffer` output where feasible, but cover the most important state transitions with assertion tests first.

### Modules to test

- **`PartRenderer`** — Pure function, easy to unit test. Feed every `Part` variant, assert the returned `Line`s have the expected styles and content. Cover all Tool states, edge cases (empty text, long step reason).
- **`KeyChord`** — State machine, easy to unit test. Test: direct binding fires immediately, leader key enters waiting state, second key completes chord, timeout cancels leader, multiple leaders stack.
- **`PromptBar`** — Unit test with simulated key events. Test: character insertion, backspace, enter, prefix detection (`/`, `!`, `@` at start-of-input changes mode), cursor movement, text extraction.
- **`App`** — Integration-level tests using `MockBackend`. Test: session auto-creation on first message, stream event handling (part updates/deltas → partial_parts, assistant MessageUpdated → commit), agent cycling, screen transition on send, modal open/close, error handling from Backend.
- **`Theme`** — Verify `Default` impl compiles and all fields have non-reset styles.

### Prior art

- `opencode-backend` mock module at `backend/src/mock.rs` — existing `MockBackend` used in tests. Follow the same pattern for TUI tests.
- `ocpncord-backend-opencode` integration tests at `ocpncord-backend-opencode/tests/integration.rs` — shows how `Backend` trait is exercised end-to-end (though TUI tests use mocking, not a real server).

### What not to test (in this phase)

- No end-to-end tests requiring a real server — those live in `ocpncord-backend-opencode/tests/` and are existing.
- No visual snapshot tests for the full terminal output — the ratatui rendering pipeline makes these fragile. Test widget-level logic instead.
- No performance/benchmark tests.

## Out of Scope

- **Theme loading from `tui.json`** — The `Theme` struct is designed for this, but loading from config is deferred. MVP uses the hardcoded TokyoNight default.
- **`/themes` slash command** — Deferred until theme loading exists.
- **`/compact` (session summarization)** — Requires the compact API endpoint; defer.
- **`/undo` / `/redo`** — Requires the revert/unrevert session API; defer.
- **`/share` / `/unshare`** — Requires the share API; defer.
- **`/export`** — Export to Markdown; defer.
- **`/editor`** — Opens `$EDITOR`; defer.
- **`/thinking` toggle** — Deferred until there is dedicated state for detail visibility.
- **`/connect`** — API key setup; defer to a future setup flow.
- **`/init`** — AGENTS.md creation; defer.
- **Drag-and-drop image attachment** — Terminal-level feature; defer.
- **Custom slash commands from `.opencode/commands/`** — Defer.
- **Embedded/mousefood target** — The `no_std` foundation is set up, but the TUI is built and tested only on desktop (crossterm). Embedded support is deferred.
- **Settings modal** — Deferred from MVP (no settings to configure yet). The modal system can be extended later.
- **`/` open command list** — Typing `/` alone to see available commands; defer.

## Further Notes

- The `Event::Backend(BackendEvent)` variant needs to be added to `tui/src/event.rs`. The `BackendEvent` type is available via the `ocpncord_backend` crate (re-exported in `tui`).
- The `native/src/main.rs` binary is the only non-`no_std` crate. It owns the crossterm terminal setup, the event translation layer (crossterm → `tui::Event::Key`), and the tokio event loop. All widget logic lives in `tui/`.
- The app-owned full-screen state is `AppMode`, not a separate Screen trait. Keep top-level mode layout in `App` and keep focused rendering helpers such as `PromptBar` and `chat.rs` behind small interfaces.
- Implementation order recommendation: (1) Event variants + Action enum, (2) KeyChord, (3) PromptBar, (4) PartRenderer, (5) App-owned StartPage/Chat mode layout, (6) `chat.rs` transcript rendering, (7) Modal trait + session list modal, (8) Wire App + event loop in native.
