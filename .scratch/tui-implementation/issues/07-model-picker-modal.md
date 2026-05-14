Status: ready-for-agent

## Parent

`.scratch/tui-implementation/PRD.md`

## What to build

Add the Model picker modal so that users can see available models and switch the active model used by the agent.

- **Model list fetching**: The Backend trait doesn't have a dedicated `list_models()` method, but `Backend::get_config()` returns a `Config` with an optional `model` field showing the currently configured model. The model picker currently shows basic information from the config — extended model listing is deferred until the Backend supports it (or can be read from the Agent list).
- **Model picker content**: A scrollable list showing the current model from config, with placeholder for full model listing. Shows the currently active model name at the top of the list with a visual indicator (e.g., `*` or highlighted row).
- **Model switching**: Selecting a model from the list updates the `Config` via a `Backend` call (not yet implemented in the Backend trait). For MVP, the model picker is view-only — the user can see the current model but cannot change it. This is clearly communicated in the UI ("Read-only: configure model via server config").
- Opens via `/models` slash command or `Ctrl+X M` keybinding.

## Acceptance criteria

- [ ] `/models` opens the model picker modal
- [ ] `Ctrl+X M` opens the model picker modal
- [ ] Model picker shows the current model from `Backend::get_config()`
- [ ] Model picker displays a note that model selection is read-only in MVP
- [ ] Model list is scrollable
- [ ] Escape closes the modal
- [ ] `cargo test -p opencode-tui` passes with tests for model picker rendering with MockBackend config

## Blocked by

`.scratch/tui-implementation/issues/05-modal-infrastructure-session-list.md`
