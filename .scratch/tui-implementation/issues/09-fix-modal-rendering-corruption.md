Status: completed

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Fix modal overlay rendering so background content (ASCII art, chat messages) does not bleed through modal borders. Currently, `CrosstermBackend.draw()` prints cell symbols via `crossterm::style::Print(symbol)` without setting terminal foreground/background colors, so the black-on-black style applied by the modal overlay's `set_style` call is never actually sent to the terminal. Result: ASCII art characters from the start page remain visible through the modal's right border and content area.

The fix cuts through two layers:

- **CrosstermBackend** (`native/src/main.rs`): The `draw()` method must emit ANSI color codes from each cell's `Style` (foreground and background) before printing the symbol. Where the style is `reset` or default, no color changes are needed.

- **Modal overlay** (`tui/src/app.rs`): Before drawing the modal border and content, clear the modal area's cell symbols (not just styles) so no stale characters from the underlying screen remain. Use `ratatui_core::widgets::Clear` or iterate the buffer to reset symbols to spaces.

## Acceptance criteria

- [ ] Modal overlays (Help, Command Palette, Sessions, Model Picker) render with clean borders — no ASCII art or chat text visible through the modal area
- [ ] `Ctrl+X H` opens Help modal with clear, readable content
- [ ] `Ctrl+P` opens Command Palette with no background bleed-through
- [ ] `Ctrl+X L` opens Sessions modal with no background bleed-through
- [ ] All existing unit tests pass (`cargo test -p ocpncord-tui --lib`)
- [ ] `cargo check --workspace` passes

## Blocked by

None — can start immediately
