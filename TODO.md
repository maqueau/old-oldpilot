# TODO

## Completed

- [x] Runtime Copilot lookup with a friendly installation error.
- [x] Single-key confirmation using Crossterm raw-mode input.
- [x] `oldpilot doctor` checks Copilot, clipboard, shell, and terminal status.
- [x] `doctor` returns a failure status when required checks fail.
- [x] README usage, configuration, safety, and development documentation.
- [x] Deterministic tests for environment precedence and executable lookup.
- [x] Interactive settings editor (`oldpilot settings`) for viewing and editing config fields, with a non-interactive fallback.
- [x] CI workflow for formatting, tests, and Clippy.

## Remaining

- [ ] Verify the current official Copilot CLI installation command and update the hint if GitHub changes it.
- [ ] Add platform-specific integration tests for raw terminal cleanup and clipboard providers.
- [ ] Consider persisting settings-editor changes automatically on quit (with a confirmation prompt) rather than requiring an explicit Save.
- [ ] Add input validation/help text in the settings editor for `confirm_execute` (currently toggled, not typed).
