Status: completed

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Implement streaming response rendering so that when the user sends a message, the agent's response appears Part-by-Part in real-time in the Chat message list.

This is the heart of the TUI experience:

- **`Event::Backend(BackendEvent)` variant**: Add to the existing `Event` enum. Carries a `BackendEvent` from the Backend trait. The native event loop polls the `PromptStream` in the `tokio::select!` alongside crossterm input and tick.
- **PartRenderer module**: A pure function that converts each `Part` variant into styled ratatui `Line`s:
  - `Text(TextPart)` → one or more `Line`s with `theme.part_text`
  - `Reasoning(ReasoningPart)` → italic/yellow `Line`s with `theme.part_reasoning`
  - `Tool(ToolPart)` → styled by state: idle (dim), running (blue, spinner), done (green, checkmark), error (red, cross). Shows tool name + state.
  - `StepStart(StepStartPart)` → dim divider line
  - `StepFinish(StepFinishPart)` → dim divider line with optional reason
  - `ToggleDetails` action controls whether tool details and reasoning are shown
- **Stream state in App**: 
  - `stream: Option<B::PromptStream>` — stored when `prompt()` returns
  - `partial_parts: Vec<Part>` — accumulated from `BackendEvent::Part` events
  - On `BackendEvent::Part { part, .. }`: append to `partial_parts`
  - On `BackendEvent::Done`: commit `partial_parts` as a new `LoadedMessage`, append to `messages`, set `stream` to `None`
  - On `BackendEvent::Error`: render as red error message in the message list
- **Chat renders streaming state**: When `stream` is `Some`, the Chat screen renders all `messages` (complete) plus the assistant's `partial_parts` as a live message at the bottom. The message list auto-scrolls to follow new content.
- **Interrupt**: Pressing Escape while a stream is active calls `Backend::abort_session()` and discards the partial stream. The partial message is NOT committed — it disappears. The PromptBar becomes interactive again.
- **PromptBar state**: While streaming, the PromptBar shows "Agent is responding..." and does not accept input. Escape interrupts.
- **`prompt()` call**: On Enter from the PromptBar, `App` calls `Backend::prompt(session_id, text)`, stores the returned stream, clears the input.
- **`/undo` and `/redo`**: Deferred (out of scope for MVP).
- **Streaming indicator**: When a stream is active, the Chat header or PromptBar shows a visual indicator (e.g., a pulsing dot or spinner).

## Acceptance criteria

- [ ] Sending a message calls `Backend::prompt()` and stores the stream
- [ ] `Event::Backend(BackendEvent::Part)` appends parts to `partial_parts` and renders them in the Chat message list
- [ ] `Event::Backend(BackendEvent::Done)` commits the partial message as a final message and renders it statically
- [ ] `Event::Backend(BackendEvent::Error)` shows a red error in the message list
- [ ] Parts render with correct styles per their variant and tool state
- [ ] Chat auto-scrolls as new content arrives
- [ ] Escape interrupts a running stream and aborts via `Backend::abort_session()`
- [ ] PromptBar shows "Agent is responding..." during streaming and is non-interactive
- [ ] `cargo test -p opencode-tui` passes with tests for:
  - PartRenderer: every Part variant produces correctly styled output
  - App stream handling: Part → partial_parts, Done → commit, Error → display
  - Interrupt flow with MockBackend
- [ ] `cargo test -p opencode-backend` still passes (no regressions in backend crate)

## Blocked by

`.scratch/tui-implementation/issues/02-send-message-chat-screen.md`
