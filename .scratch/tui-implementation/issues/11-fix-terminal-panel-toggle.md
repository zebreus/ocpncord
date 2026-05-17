Status: completed

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Fix the terminal panel toggle: `Ctrl+X T` currently maps to `Action::OpenTerminal(String::new())` which unconditionally sets `active_screen = ScreenId::Terminal` with no way to return to the previous screen via keyboard. When no PTY has been created by the agent, the terminal screen shows only a status bar and empty content. There is no keybinding to close the terminal panel and return to StartPage or Chat.

The fix:

- Change `Ctrl+X T` from `Action::OpenTerminal` to a **toggle**: if already on `ScreenId::Terminal`, return to the previous screen (`StartPage` or `Chat`); otherwise switch to `ScreenId::Terminal`. Store the previous screen ID in `App` when switching to Terminal.
- If the terminal screen is entered and no PTY has been created (`self.terminal.pty_id.is_none()`), show a helpful message like "No active terminal — send a prompt to the agent to create one" instead of a blank screen.
- Add `Ctrl+X T` support in the Terminal screen to toggle back.

## Acceptance criteria

- [ ] `Ctrl+X T` on StartPage switches to Terminal screen
- [ ] `Ctrl+X T` again on Terminal screen returns to StartPage (or Chat if previously there)
- [ ] Terminal screen with no active PTY shows a helpful message, not a blank/empty pane
- [ ] Terminal screen with an active PTY renders PTY output with the status bar at the bottom
- [ ] All existing unit tests pass (`cargo test -p ocpncord-tui --lib`)
- [ ] `cargo check --workspace` passes

## Blocked by

None — can start immediately
