Status: ready-for-agent

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Add the Command palette modal so that users can search and execute any available action without memorising keybindings or slash commands.

This mirrors the `Ctrl+P` command palette from the official opencode TUI.

- **Command palette content**: A searchable list of all available actions. Each item has a display name, the action it triggers, and the keybinding (if any). MVP command list:
  - Help (`/help`, `Ctrl+X H`)
  - New Session (`/new`, `Ctrl+X N`)
  - Sessions (`/sessions`, `Ctrl+X L`)
  - Models (`/models`, `Ctrl+X M`)
  - Exit (`/exit`, `Ctrl+X Q`)
  - Toggle Details (`/details`)
  - Cycle Agent (`Tab`)
- **Search**: As the user types in the palette search input, the list filters to matching items (fuzzy or prefix match on display name or slash command). The search input is a single-line text field at the top of the palette.
- **Selection**: Arrow keys navigate the filtered list. Enter executes the selected command (dispatches the same `Action` as the keybinding would). Escape closes the palette without executing.
- **Layout**: The palette takes roughly the centre of the screen (not full width like other modals — the official TUI uses a compact centred box). Shows "Command Palette" as the title, a search input line, and the filtered list below.
- Opens via `Ctrl+P` keybinding only (no slash command — `/` is reserved for the main input).

## Acceptance criteria

- [ ] `Ctrl+P` opens the command palette modal
- [ ] Palette shows a search input at the top and a scrollable list of commands below
- [ ] Typing in the search input filters the command list
- [ ] Arrow keys navigate the filtered list
- [ ] Enter executes the selected command (same Action as triggering the command normally)
- [ ] Escape closes the palette without executing
- [ ] All MVP commands are listed with their slash command and keybinding
- [ ] `cargo test -p opencode-tui` passes with tests for palette filtering and command execution

## Blocked by

`.scratch/tui-implementation/issues/05-modal-infrastructure-session-list.md`
