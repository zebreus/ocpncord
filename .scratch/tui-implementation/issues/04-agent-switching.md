Status: completed

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Add agent awareness to the TUI so that users can switch between primary agents (Build, Plan, or custom agents) using Tab, and see which agent is active in the PromptBar indicator.

- **`Backend::list_agents()` on launch**: When `App` is created, it calls `Backend::list_agents()`. The result is filtered to only `mode: "primary"` agents. These are stored in `App.agents: Vec<Agent>` and `App.active_agent: usize` (index).
- **Tab cycling**: Pressing Tab calls `App.cycle_agent()`, which increments `active_agent` (wrapping around). `Shift+Tab` decrements. This works on both StartPage and Chat.
- **Agent indicator in PromptBar**: The right side of the PromptBar shows the active agent's name. The colour/style of the indicator varies per agent:
  - Build → green (`theme.agent_indicator` with green tint)
  - Plan → yellow/orange tint
  - Custom agents → default `theme.agent_indicator` colour
- **Agent sent with prompt**: When `Backend::submit_prompt()` or `Backend::submit_command()` is called, the active agent name is passed as the `agent` parameter. The model is not overridden — the agent's default model from the server config is used.
- **Agent list as a data dependency**: If `list_agents()` fails on startup, the TUI defaults to a hardcoded list `["build", "plan"]` and continues. A warning is logged but no error is shown to the user.
- **Display fallback**: If no agents are returned (empty list), the indicator shows "build" as a sensible default.

## Acceptance criteria

- [ ] `Backend::list_agents()` is called on startup; primary agents are stored in App state
- [ ] Tab cycles through primary agents forward, Shift+Tab cycles backward
- [ ] Active agent name is displayed in the PromptBar indicator on both StartPage and Chat
- [ ] Agent name is passed to `Backend::submit_prompt()` when sending a message
- [ ] If `list_agents()` fails, defaults to `["build", "plan"]` with a warning only
- [ ] If agent list is empty, indicator shows "build"
- [ ] `cargo test -p ocpncord-tui` passes with tests for:
  - Agent cycling logic
  - Agent parameter passed through to `submit_prompt()`
  - MockBackend returns agent list, TUI uses it
  - Failure fallback behaviour

## Blocked by

`.scratch/tui-implementation/issues/03-streaming-responses.md`
