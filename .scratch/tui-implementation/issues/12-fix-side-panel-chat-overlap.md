Status: completed

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

When the side panel (Diagnostics/Todos/Terminal) is visible (toggled via `Ctrl+X D` or `Ctrl+X O`), chat messages render underneath the panel on the right ~30% of the screen. This clips message text horizontally — the right portion of each message line is hidden behind the side panel content.

Root cause: `render_chat()` and `StartPage.render()` both render into the full `frame.area()` without accounting for the side panel's width. The side panel is rendered later on top, but the underlying text pixels are still sent to the terminal and partially overwritten.

Fix: when `side_panel_visible` is true, reduce the area passed to the main screen's render by the side panel width (30% of terminal width). This ensures:
- `render_chat()` only writes to the left 70% of the terminal
- `StartPage.render()` centers the logo within the reduced width
- The PromptBar renders within the reduced width
- The side panel renders in the right 30% without overlapping

## Acceptance criteria

- [ ] With side panel visible, chat messages don't extend under the panel — text is clipped at the panel boundary
- [ ] With side panel visible, StartPage logo is centered in the left 70% of the terminal
- [ ] With side panel hidden, all content uses full terminal width (existing behavior preserved)
- [ ] Toggling side panel on/off reflows content correctly
- [ ] All existing unit tests pass (`cargo test -p ocpncord-tui --lib`)
- [ ] `cargo check --workspace` passes

## Blocked by

None — can start immediately
