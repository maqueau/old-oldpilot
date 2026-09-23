# TODO

## Completed on `todo-implementation`

- [x] Add a `doctor` subcommand that reports Copilot, clipboard, and terminal status.
- [x] Add a friendly missing-Copilot installation hint.
- [x] Add basic tests for confirmation parsing and command lookup.
- [x] Add initial usage and configuration documentation in `README.md`.

## Remaining implementation work

- [ ] Resolve the Copilot binary once through a shared path-resolution routine and make both normal execution and `doctor` honor `COPILOT_BIN`, `copilot_bin`, and explicit paths consistently.
- [ ] Implement confirmation with Crossterm raw-mode key events so `y` works without Enter; always restore terminal state on success and error paths.
- [ ] Make `doctor` test clipboard capability using the same `arboard`-first behavior as the copy action, and return a nonzero status when required checks fail.
- [ ] Make `doctor` check the configured shell as well as Copilot and clipboard support.
- [ ] Add deterministic tests for configuration precedence, binary resolution, doctor status, and failure cases without requiring a real Copilot installation or interactive terminal.
- [ ] Verify the current official Copilot CLI installation instructions and keep the README/error hint aligned with them.
- [ ] Review command execution and terminal cleanup for interruption/error paths.
