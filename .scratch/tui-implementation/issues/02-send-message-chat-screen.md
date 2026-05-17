Status: finished

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Add the PromptBar widget (text input with live prefix detection) and the Chat screen so that starting from the StartPage, the user can type a message and press Enter to transition to the Chat screen.

This is the core interaction flow. It cuts through:

- **PromptBar**: A ratatui widget providing text input. Supports character insertion, backspace, cursor movement, and Enter. Performs live detection of the first non-whitespace character: `/` (command mode — changes PromptBar appearance), `!` (shell mode), `@` (file reference), or none (normal text). The visual style of the PromptBar changes based on the detected mode (e.g., different border colour or prefix icon). Also shows the active agent name and model on the right side of the input area (placeholder values for now).
- **StartPage gains PromptBar**: The PromptBar is centred below the logo. Typing Enter submits the input.
- **Session auto-creation**: When the user presses Enter and `App.active_session` is `None`, `App` calls `Backend::create_session()` with a default title and the current directory. If this fails, an error message is displayed (red, via `theme.text_error`).
- **Chat screen**: A full-screen view with a message list area (empty, showing a placeholder like "No messages yet") and the PromptBar docked to the bottom. The message list is scrollable (placeholder — no real messages until slice 3).
- **Screen transition**: On successful session creation + message submission (or just session creation — the message itself is sent in slice 3), the active screen switches from `StartPage` to `Chat`.
- **PromptBar submits**: Right now, pressing Enter only transitions the screen and creates the session; no prompt is sent to the Backend yet (that's slice 3). The typed text is stored in `App.draft` to be used by slice 3.
- **Error handling**: If `Backend::create_session()` fails, the error is displayed as a red message overlay that auto-dismisses on the next keypress. The user stays on StartPage.
- **`/new` slash command**: Typing `/new` and pressing Enter on the Chat screen transitions back to StartPage (or clears state and stays on Chat — a new session is created on next message).

## Acceptance criteria

- [ ] StartPage shows the PromptBar centred below the logo
- [ ] Typing characters shows them in the PromptBar; backspace deletes
- [ ] Typing `/`, `!`, or `@` at the start of input changes the PromptBar's visual appearance
- [ ] Pressing Enter on StartPage with non-empty text transitions to Chat screen
- [ ] Chat screen shows the PromptBar docked at the bottom and an empty message list above
- [ ] A new session is created via `Backend::create_session()` on first message
- [ ] If session creation fails, a red error is shown and the user stays on StartPage
- [ ] `/new` on Chat screen creates a fresh session and resets the view
- [ ] `cargo test -p ocpncord-tui` passes with tests for:
  - PromptBar character entry, backspace, prefix detection
  - App session auto-creation flow with MockBackend
  - Screen transition on message submit

## Blocked by

`.scratch/tui-implementation/issues/01-minimal-render-loop-startpage.md`
