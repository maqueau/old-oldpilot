# oldpilot

`oldpilot` is a small Rust wrapper around the GitHub Copilot CLI that restores the deprecated command-oriented `suggest` and `explain` workflows.

## Requirements

- Rust 2021 toolchain
- GitHub Copilot CLI installed and authenticated
- An interactive terminal for `cs`, `csx`, and `settings`
- Clipboard support for the copy action: `arboard` may use the desktop environment; the fallback commands are `pbcopy` (macOS), `wl-copy` (Wayland), or `xclip` (X11)

The current implementation is intended for Unix-like environments (macOS/Linux). The shell helper specifically targets Zsh.

## Build and run

```sh
cargo build --release
./target/release/oldpilot ce "explain this command"
./target/release/oldpilot cex "explain this command"
./target/release/oldpilot cs "find files larger than 100 MB"
./target/release/oldpilot csx "show running Docker containers"
./target/release/oldpilot settings
./target/release/oldpilot doctor
```

`ce` and `cs` use the configured default model. `cex` and `csx` let the Copilot CLI choose its model.

## Installation and Copilot CLI

Install the official Copilot CLI using the current GitHub instructions, then verify that the executable is on `PATH`:

```sh
command -v copilot
copilot --version
```

Build the wrapper and place it somewhere on `PATH` if desired:

```sh
cargo install --path .
```

## Configuration

The configuration file is read from `$XDG_CONFIG_HOME/oldpilot/config`, or from `~/.config/oldpilot/config` when `XDG_CONFIG_HOME` is unset. It uses simple `key=value` lines:

```text
default_model="gpt-4.1"
confirm_execute=true
copilot_bin="copilot"
shell="sh"
```

Environment variables take precedence over file values:

- `COPILOT_DEFAULT_MODEL`
- `COPILOT_CS_CONFIRM_EXECUTE` (`true`/`false`, `1`/`0`, or `yes`/`no`)
- `COPILOT_BIN`
- `COPILOT_SHELL`

Run `oldpilot settings` to edit the file interactively.

## Safety behavior

A suggestion is never executed automatically. `cs` and `csx` show the generated command in an action menu. The user must select **Execute**, **Copy**, **Explain**, or **Quit**. Execution confirmation is enabled by default and can be disabled with `confirm_execute=false` or `COPILOT_CS_CONFIRM_EXECUTE=false`.

Generated commands are passed to the configured shell with `-c`; review them carefully before execution.

## Zsh helpers

The legacy helper script provides `cs`, `csx`, `ce`, and `cex` shell functions:

```sh
source copilot-cli-suggest-explain.zsh
cs "list running processes"
ce "explain ps aux"
```

The script uses the same command prefixes as the Rust program and supports `pbcopy`, `wl-copy`, and `xclip` for copying.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

`TODO.md` tracks remaining runtime hardening and documentation work.
