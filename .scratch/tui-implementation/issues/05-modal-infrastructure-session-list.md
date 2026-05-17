Status: finished

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Add the modal overlay system and the Session list modal so that users can browse, select, and delete sessions without leaving the current screen.

- **Modal trait**: A trait mirroring `Screen` but designed for overlay rendering:
  ```rust
  pub trait Modal {
      fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect);
      fn handle_event(&mut self, event: Event) -> Action;
      fn title(&self) -> &str;
  }
  ```
- **Overlay rendering in App**: `App::render()` renders the base screen first, then if `active_modal: Option<Box<dyn Modal>>` is `Some`, it:
  1. Uses `ratatui_core::widgets::Clear` to fill the entire terminal with a dimmed overlay (semi-transparent black using `theme.modal_overlay`)
  2. Calculates a centred rectangle (60% width, 70% height) and renders the modal inside it
- **ModalId enum**: Identifiers for each modal type: `SessionList`, `Help`, `ModelPicker`, `CommandPalette`, `Settings` (Settings is placeholder for now).
- **Modal open/close**: Opening a modal sets `App.active_modal`. Escape or an explicit `Action::CloseModal` clears it. Only one modal can be open at a time.
- **Session list modal**: Opens via `/sessions` or `Ctrl+X L`. Shows a scrollable list of sessions from `Backend::list_sessions()`. Each row shows session title, date, and message count. Arrow keys to navigate, Enter to select (loads into Chat and switches to Chat screen), Delete to confirm-delete (calls `Backend::delete_session()`). A loading state is shown while sessions are being fetched. An empty state ("No sessions yet") is shown if the list is empty.
- **`/new` command**: Creates a new session via `Backend::create_session()`, switches to Chat screen with the new session, closes any open modal.
- **PromptBar `/` routing**: Typing `/sessions`, `/new` in the PromptBar and pressing Enter dispatches to the corresponding modal/screen action rather than submitting as a message. The PromptBar handles this by checking if the input starts with `/` and matching against known commands. Unknown `/` commands are submitted as messages to the agent (the agent handles them on the server side).

## Acceptance criteria

- [ ] Modal trait compiles and works with overlay rendering (dimmed background + centred rectangle)
- [ ] Escape closes the active modal
- [ ] `/sessions` and `Ctrl+X L` open the session list modal
- [ ] Session list shows sessions from `Backend::list_sessions()` (or "No sessions yet" if empty)
- [ ] Arrow keys navigate the session list, Enter selects and loads the session into Chat
- [ ] Delete key prompts for confirmation, then calls `Backend::delete_session()`
- [ ] `/new` creates a new session and switches to Chat
- [ ] Session list shows a loading state while fetching
- [ ] Unknown `/` commands are submitted as regular messages to the agent
- [ ] `cargo test -p ocpncord-tui` passes with tests for:
  - Modal trait implementation
  - Session list rendering with MockBackend (sessions present, empty, error states)
  - Session selection loads messages correctly
  - `/new` flow with MockBackend
  - `/` command routing (known commands dispatch, unknown commands submit)

## Blocked by

`.scratch/tui-implementation/issues/02-send-message-chat-screen.md`
