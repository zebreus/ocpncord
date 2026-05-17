Status: completed

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

When the agent is streaming a response (`is_streaming == true`), pressing `Ctrl+C` currently falls through the streaming check and is handled by `KeyChord`, which emits `Action::Quit` and exits the application. The intended behavior per PRD user stories #16 and #40 is that Escape interrupts streaming — but `Ctrl+C` should either:

- Also interrupt the stream (same as Escape), or
- Be explicitly ignored during streaming

Worse: if streaming finishes between the user pressing `Ctrl+C` and the event being processed, `is_streaming` is false and `Ctrl+C` passes through to `KeyChord` → `Action::Quit`, unexpectedly closing the app.

Fix: in `App::handle_event()`, during the streaming branch (`is_streaming == true`), add explicit handling for `Ctrl+C` alongside Escape. Both should call `handle_interrupt()`. This is consistent with the reference opencode TUI where `Ctrl+C` interrupts the current operation.

Also, outside streaming mode, `Ctrl+C` should still quit the app (existing behavior) — no change there.

## Acceptance criteria

- [ ] During streaming, `Ctrl+C` interrupts the agent (same as Escape)
- [ ] During streaming, `Escape` continues to interrupt the agent (existing behavior preserved)
- [ ] Outside streaming, `Ctrl+C` still quits the application (existing behavior preserved)
- [ ] Aborted/error streams don't deadlock or leave stale state
- [ ] All existing unit tests pass (`cargo test -p ocpncord-tui --lib`)
- [ ] `cargo check --workspace` passes

## Blocked by

None — can start immediately
