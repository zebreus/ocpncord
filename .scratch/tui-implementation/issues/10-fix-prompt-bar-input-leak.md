Status: completed

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Fix the prompt bar input leak: after closing a modal (via Escape or explicit action), leftover characters from previous interactions remain visible in the prompt bar's render area. Observed as `> x` after closing Help modal, and `xxxx/help` when typing after closing the Command Palette.

Root cause: when a modal is dismissed, `prompt_bar.clear()` is not always called. The prompt bar's internal `input` string retains characters typed before the modal opened, and these render on top of the modal-confused input state.

Fix: ensure `prompt_bar.clear()` is called when any modal is closed, regardless of close path (Escape, action button, or programmatic close). The single point of fix is in `App::handle_event()` where `active_modal` is set to `None` — add `self.prompt_bar.clear()` there.

## Acceptance criteria

- [ ] Open Help modal via `Ctrl+X H`, close via Escape — prompt bar shows `> ` with no leftover characters
- [ ] Open Command Palette via `Ctrl+P`, close via Escape — prompt bar shows `> ` with no leftover characters
- [ ] Open Sessions modal via `Ctrl+X L`, close via Escape — prompt bar shows `> ` with no leftover characters
- [ ] Type `hello`, open and close Help modal — prompt bar shows `> hello` (input preserved), not `> xhello` or `> hellohello`
- [ ] All existing unit tests pass (`cargo test -p ocpncord-tui --lib`)
- [ ] `cargo check --workspace` passes

## Blocked by

None — can start immediately
