Status: ready-for-agent

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Add the Help modal so that users can open a reference of available slash commands and keybindings from within the TUI.

- **Help modal content**: A scrollable list of commands grouped by category:
  - **Slash commands**: `/help`, `/sessions`, `/new`, `/models`, `/exit`, `/details`
  - **Keybindings**: `Ctrl+X H` (help), `Ctrl+X Q` (quit), `Ctrl+X N` (new session), `Ctrl+X L` (sessions), `Ctrl+X M` (models), `Ctrl+P` (palette), `Tab`/`Shift+Tab` (cycle agent), `Escape` (close modal / interrupt)
  - **Input prefixes**: `/` (command), `!` (shell), `@` (file reference)
- All content is hardcoded in this slice — no need to read from a file or server. The command/keybinding list matches the MVP scope from the PRD.
- Opens via `/help` slash command or `Ctrl+X H` keybinding.

## Acceptance criteria

- [ ] `/help` opens the help modal
- [ ] `Ctrl+X H` opens the help modal
- [ ] Help modal lists all MVP slash commands with descriptions
- [ ] Help modal lists all MVP keybindings with descriptions
- [ ] Help modal lists input prefix modes
- [ ] Content is scrollable if it exceeds the modal area
- [ ] Escape closes the modal
- [ ] `cargo test -p opencode-tui` passes with tests for help modal rendering

## Blocked by

`.scratch/tui-implementation/issues/05-modal-infrastructure-session-list.md`
