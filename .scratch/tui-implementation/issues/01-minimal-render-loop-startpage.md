Status: finished

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Wire up the foundation of the TUI so that running the binary shows the StartPage with the opencode ASCII art logo and a tip line, and the user can quit with `Ctrl+X Q`.

This slice cuts through every layer:

- **Event system**: Add `Event::Tick` variant to the existing `Event` enum so the render loop has a pacing mechanism.
- **Action enum**: Replace the stub `Action` with a rich enum covering `None`, `Quit`, `SwitchScreen(ScreenId)`, `OpenModal(ModalId)`, `CloseModal`, `CycleAgent`, `Interrupt`, `SendMessage`, `OpenPalette`, `ScrollUp`, `ScrollDown`, `ToggleDetails`. For this slice only `None` and `Quit` need real handling — the rest are placeholders.
- **KeyChord**: Implement the leader-key state machine. `Ctrl+X` starts leader mode; the next non-modifier key within 2 seconds completes the chord. `Ctrl+X Q` → `Action::Quit`. Direct bindings like `Ctrl+C` → `Action::Quit` also work. A `Tick` event while leader is active checks the timeout.
- **Theme** (already done): Verify the `Theme` struct with its TokyoNight default compiles and all render paths receive a `&Theme`.
- **StartPage**: A full-screen widget that renders the opencode ASCII art logo centred vertically, with a tip line underneath ("Tip: Ctrl+X H for help, Ctrl+X Q to quit"). No PromptBar yet.
- **App**: Holds `active_screen: ScreenId` (defaults to `StartPage`), `theme: Theme`, `key_chord: KeyChord`. `handle_event()` dispatches `Event::Key` through `KeyChord` → `Action`, and `Action::Quit` returns a quit signal. `render()` delegates to the active screen's `render()`.
- **native event loop**: `tokio::select!` racing `crossterm::event::read()` (translated to `tui::Event::Key`), and a 50ms tick → `Event::Tick`. After each event, `app.handle_event()` + `terminal.draw()`. Loop exits on quit signal. Crossterm raw mode + alternate screen on start, restored on exit.
- **ScreenId**: Has exactly two variants — `StartPage` and `Chat`. `ScreenId::SessionList` is removed (sessions are now a modal).

## Acceptance criteria

- [ ] Running the binary shows the StartPage with logo and tip, on a clear terminal (alternate screen)
- [ ] `Ctrl+X Q` quits the application and restores the terminal
- [ ] `Ctrl+C` also quits the application
- [ ] `cargo check --workspace` passes
- [ ] `cargo test -p ocpncord-tui` passes with at least basic tests for KeyChord (direct binding, leader+chord, timeout)

## Blocked by

None — can start immediately
