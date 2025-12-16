use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};
use ratatui::Terminal;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const CS_PREFIX: &str = "[[CPLT:CS]]";
const CE_PREFIX: &str = "[[CPLT:CE]]";
const DEFAULT_MODEL: &str = "gpt-4.1";

#[derive(Parser)]
#[command(
    name = "oldpilot",
    version,
    about = "oldpilot - Copilot CLI helper",
    long_about = "Copilot CLI wrapper that brings back the deprecated feature set of explaining and suggesting commands in the terminal using the official Copilot CLI."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Explain a command using the default model
    Ce { prompt: Vec<String> },
    /// Explain a command using the Copilot default model selection
    Cex { prompt: Vec<String> },
    /// Suggest a command then act using the default model
    Cs { prompt: Vec<String> },
    /// Suggest a command using the Copilot default model selection
    Csx { prompt: Vec<String> },
    /// Open an interactive settings menu (writes ~/.config/oldpilot/config)
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Execute,
    Copy,
    Explain,
    Quit,
}

#[derive(Clone, Debug, Default)]
struct Config {
    /// Default model to use when the user runs `ce`/`cs` (overridable by env)
    default_model: Option<String>,
    /// Whether to prompt before executing suggested commands (overridable by env)
    confirm_execute: Option<bool>,
    /// Which `copilot` binary to execute (overridable by env)
    copilot_bin: Option<String>,
    /// Which shell to run commands through (overridable by env)
    shell: Option<String>,
}

fn config_path() -> Result<PathBuf> {
    // Prefer XDG-ish config location
    if let Ok(dir) = env::var("XDG_CONFIG_HOME") {
        let mut p = PathBuf::from(dir);
        p.push("oldpilot");
        p.push("config");
        return Ok(p);
    }

    let home = env::var("HOME").context("HOME is not set")?;
    let mut p = PathBuf::from(home);
    p.push(".config");
    p.push("oldpilot");
    p.push("config");
    Ok(p)
}

fn load_config() -> Result<Config> {
    let path = config_path()?;
    let data = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(anyhow!(e)).context("failed to read config"),
    };

    let mut cfg = Config::default();
    for raw in data.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let mut val = v.trim().to_string();
        // Allow quoted strings
        if (val.starts_with('"') && val.ends_with('"'))
            || (val.starts_with('\'') && val.ends_with('\''))
        {
            if val.len() >= 2 {
                val = val[1..val.len() - 1].to_string();
            }
        }

        match key {
            "default_model" => {
                if !val.is_empty() {
                    cfg.default_model = Some(val);
                }
            }
            "confirm_execute" => {
                let b = val.eq_ignore_ascii_case("true")
                    || val == "1"
                    || val.eq_ignore_ascii_case("yes");
                let f = val.eq_ignore_ascii_case("false")
                    || val == "0"
                    || val.eq_ignore_ascii_case("no");
                if b {
                    cfg.confirm_execute = Some(true);
                }
                if f {
                    cfg.confirm_execute = Some(false);
                }
            }
            "copilot_bin" => {
                if !val.is_empty() {
                    cfg.copilot_bin = Some(val);
                }
            }
            "shell" => {
                if !val.is_empty() {
                    cfg.shell = Some(val);
                }
            }
            _ => {}
        }
    }

    Ok(cfg)
}

fn save_config(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    let dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .context("config path has no parent")?;
    fs::create_dir_all(&dir).context("failed to create config directory")?;

    let mut out = String::new();
    out.push_str("# oldpilot config\n");
    out.push_str("# Lines are key=value. Strings may be quoted.\n\n");

    if let Some(m) = &cfg.default_model {
        out.push_str(&format!("default_model=\"{}\"\n", m.replace('"', "\\\"")));
    }
    if let Some(b) = cfg.confirm_execute {
        out.push_str(&format!(
            "confirm_execute={}\n",
            if b { "true" } else { "false" }
        ));
    }
    if let Some(b) = &cfg.copilot_bin {
        out.push_str(&format!("copilot_bin=\"{}\"\n", b.replace('"', "\\\"")));
    }
    if let Some(s) = &cfg.shell {
        out.push_str(&format!("shell=\"{}\"\n", s.replace('"', "\\\"")));
    }

    fs::write(&path, out).context("failed to write config")?;
    Ok(())
}

fn cfg_string_display(v: &Option<String>, fallback: &str) -> String {
    v.as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn cfg_bool_display(v: Option<bool>, fallback: bool) -> String {
    match v {
        Some(true) => "true".to_string(),
        Some(false) => "false".to_string(),
        None => if fallback { "true" } else { "false" }.to_string(),
    }
}

fn resolve_default_model(cfg: &Config) -> String {
    // env wins, then config, then DEFAULT_MODEL
    if let Ok(v) = env::var("COPILOT_DEFAULT_MODEL") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    cfg.default_model
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

fn resolve_confirm_execute(cfg: &Config) -> bool {
    // env wins, then config, then true
    if let Ok(v) = env::var("COPILOT_CS_CONFIRM_EXECUTE") {
        if v.eq_ignore_ascii_case("false") || v == "0" || v.eq_ignore_ascii_case("no") {
            return false;
        }
        if v.eq_ignore_ascii_case("true") || v == "1" || v.eq_ignore_ascii_case("yes") {
            return true;
        }
    }
    cfg.confirm_execute.unwrap_or(true)
}

fn resolve_copilot_bin(cfg: &Config) -> String {
    if let Ok(v) = env::var("COPILOT_BIN") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    cfg.copilot_bin
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "copilot".to_string())
}

fn resolve_shell(cfg: &Config) -> String {
    if let Ok(v) = env::var("COPILOT_SHELL") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    cfg.shell
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "sh".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsField {
    DefaultModel,
    ConfirmExecute,
    CopilotBin,
    Shell,
    Save,
    Quit,
}

fn run_settings() -> Result<()> {
    let mut cfg = load_config()?;

    let mut stdout = io::stdout();
    enable_raw_mode().context("enable raw mode")?;
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let result: Result<()> = (|| {
        let fields = [
            SettingsField::DefaultModel,
            SettingsField::ConfirmExecute,
            SettingsField::CopilotBin,
            SettingsField::Shell,
            SettingsField::Save,
            SettingsField::Quit,
        ];

        let mut idx = 0usize;
        let mut status_line = String::from("↑/↓ move · Enter edit/toggle · s save · q quit");

        // Inline editor state
        let mut editing: Option<SettingsField> = None;
        let mut edit_buf = String::new();

        loop {
            terminal
                .draw(|f| {
                    let area = f.size().inner(&Margin {
                        horizontal: 2,
                        vertical: 1,
                    });

                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(3),
                            Constraint::Min(6),
                            Constraint::Length(2),
                        ])
                        .split(area);

                    let title = Paragraph::new(Line::from(vec![
                        Span::styled(
                            "oldpilot settings",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  "),
                        Span::styled(
                            format!("(config: {})", config_path().unwrap_or_default().display()),
                            Style::default().fg(Color::Gray),
                        ),
                    ]));
                    f.render_widget(title, chunks[0]);

                    let effective_model = resolve_default_model(&cfg);
                    let effective_confirm = resolve_confirm_execute(&cfg);
                    let effective_bin = resolve_copilot_bin(&cfg);
                    let effective_shell = resolve_shell(&cfg);

                    let lines: Vec<Line> = fields
                        .iter()
                        .enumerate()
                        .map(|(i, field)| {
                            let selected = i == idx;
                            let base_style = if selected {
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default()
                            };

                            let (label, value, hint) = match field {
                                SettingsField::DefaultModel => (
                                    "Default model",
                                    cfg_string_display(&cfg.default_model, &effective_model),
                                    "string",
                                ),
                                SettingsField::ConfirmExecute => (
                                    "Confirm execute",
                                    cfg_bool_display(cfg.confirm_execute, effective_confirm),
                                    "toggle",
                                ),
                                SettingsField::CopilotBin => (
                                    "Copilot binary",
                                    cfg_string_display(&cfg.copilot_bin, &effective_bin),
                                    "string",
                                ),
                                SettingsField::Shell => (
                                    "Shell",
                                    cfg_string_display(&cfg.shell, &effective_shell),
                                    "string",
                                ),
                                SettingsField::Save => ("Save", String::new(), "s"),
                                SettingsField::Quit => ("Quit", String::new(), "q"),
                            };

                            let is_editing = editing.is_some() && editing.unwrap() == *field;

                            let value_for_render = if is_editing {
                                edit_buf.clone()
                            } else {
                                value.clone()
                            };

                            let value_span = if is_editing {
                                Span::styled(
                                    value_for_render.clone(),
                                    Style::default()
                                        .fg(Color::Yellow)
                                        .add_modifier(Modifier::BOLD),
                                )
                            } else {
                                Span::styled(
                                    value_for_render.clone(),
                                    Style::default().fg(Color::Gray),
                                )
                            };

                            let left = if selected { " > " } else { "   " };

                            // Format: label  value  (hint)
                            let mut spans = vec![Span::raw(left), Span::styled(label, base_style)];

                            if !value_for_render.is_empty() {
                                spans.push(Span::raw("  "));
                                spans.push(value_span);
                            }

                            spans.push(Span::raw("  "));
                            spans.push(Span::styled(
                                format!("({})", hint),
                                Style::default().fg(Color::DarkGray),
                            ));

                            Line::from(spans)
                        })
                        .collect();

                    let list = Paragraph::new(lines).block(
                        Block::default()
                            .borders(Borders::NONE)
                            .padding(Padding::zero()),
                    );
                    f.render_widget(list, chunks[1]);

                    let status = Paragraph::new(Line::from(Span::styled(
                        status_line.clone(),
                        Style::default().fg(Color::Gray),
                    )));
                    f.render_widget(status, chunks[2]);
                })
                .context("draw settings TUI")?;

            if crossterm::event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    // If we're editing a string field, capture text input.
                    if let Some(field) = editing {
                        match key.code {
                            KeyCode::Esc => {
                                editing = None;
                                edit_buf.clear();
                                status_line = "edit canceled".to_string();
                            }
                            KeyCode::Enter => {
                                let new_val = edit_buf.trim().to_string();
                                match field {
                                    SettingsField::DefaultModel => {
                                        cfg.default_model = if new_val.is_empty() {
                                            None
                                        } else {
                                            Some(new_val)
                                        };
                                    }
                                    SettingsField::CopilotBin => {
                                        cfg.copilot_bin = if new_val.is_empty() {
                                            None
                                        } else {
                                            Some(new_val)
                                        };
                                    }
                                    SettingsField::Shell => {
                                        cfg.shell = if new_val.is_empty() {
                                            None
                                        } else {
                                            Some(new_val)
                                        };
                                    }
                                    _ => {}
                                }
                                editing = None;
                                edit_buf.clear();
                                status_line = "updated (press s to save)".to_string();
                            }
                            KeyCode::Backspace => {
                                edit_buf.pop();
                            }
                            KeyCode::Char(c) => {
                                // Basic guard: avoid control chars
                                if !c.is_control() {
                                    edit_buf.push(c);
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Up => {
                            if idx == 0 {
                                idx = fields.len() - 1;
                            } else {
                                idx -= 1;
                            }
                        }
                        KeyCode::Down => {
                            idx = (idx + 1) % fields.len();
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            break;
                        }
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            save_config(&cfg)?;
                            status_line = "saved".to_string();
                        }
                        KeyCode::Enter => match fields[idx] {
                            SettingsField::ConfirmExecute => {
                                let current =
                                    cfg.confirm_execute.unwrap_or(resolve_confirm_execute(&cfg));
                                cfg.confirm_execute = Some(!current);
                                status_line = "toggled (press s to save)".to_string();
                            }
                            SettingsField::DefaultModel => {
                                editing = Some(SettingsField::DefaultModel);
                                edit_buf = cfg.default_model.clone().unwrap_or_default();
                                status_line =
                                    "editing default_model (Enter to apply, Esc to cancel)"
                                        .to_string();
                            }
                            SettingsField::CopilotBin => {
                                editing = Some(SettingsField::CopilotBin);
                                edit_buf = cfg.copilot_bin.clone().unwrap_or_default();
                                status_line = "editing copilot_bin (Enter to apply, Esc to cancel)"
                                    .to_string();
                            }
                            SettingsField::Shell => {
                                editing = Some(SettingsField::Shell);
                                edit_buf = cfg.shell.clone().unwrap_or_default();
                                status_line =
                                    "editing shell (Enter to apply, Esc to cancel)".to_string();
                            }
                            SettingsField::Save => {
                                save_config(&cfg)?;
                                status_line = "saved".to_string();
                            }
                            SettingsField::Quit => {
                                break;
                            }
                        },
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    })();

    cleanup_terminal(&mut terminal)?;
    result
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Ce { prompt } => run_ce(prompt, Some(default_model()?.as_str())),
        Commands::Cex { prompt } => run_ce(prompt, None),
        Commands::Cs { prompt } => run_cs(prompt, Some(default_model()?.as_str())),
        Commands::Csx { prompt } => run_cs(prompt, None),
        Commands::Settings => run_settings(),
    }
}

fn run_ce(parts: Vec<String>, model: Option<&str>) -> Result<()> {
    let prompt_text = parts.join(" ");
    if prompt_text.trim().is_empty() {
        return Err(anyhow!("Prompt cannot be empty"));
    }

    let prefixed = format!("{CE_PREFIX} {prompt_text}");
    let answer = copilot_prompt(&prefixed, model)?;
    print!("{}", answer);
    Ok(())
}

fn run_cs(parts: Vec<String>, model: Option<&str>) -> Result<()> {
    let prompt_text = parts.join(" ");
    if prompt_text.trim().is_empty() {
        return Err(anyhow!("Prompt cannot be empty"));
    }

    let prompt = format!("{CS_PREFIX} TASK: {prompt_text}");
    let suggestion = copilot_prompt(&prompt, model)?;
    let trimmed = suggestion.trim();
    if trimmed.is_empty() {
        println!("No suggestion returned.");
        return Ok(());
    }

    print_block("Copilot", trimmed);

    let action = action_picker(trimmed)?;
    match action {
        Action::Execute => {
            if confirm_execute(trimmed)? {
                run_shell(trimmed)?;
            } else {
                println!("Canceled.");
            }
        }
        Action::Copy => {
            copy_to_clipboard(trimmed)?;
            println!("Copied to clipboard.");
        }
        Action::Explain => {
            let explain_prompt = format!("Explain this command: {trimmed}");
            run_ce(vec![explain_prompt], model)?;
        }
        Action::Quit => {
            println!("Canceled.");
        }
    }

    Ok(())
}

fn default_model() -> Result<String> {
    let cfg = load_config().unwrap_or_default();
    Ok(resolve_default_model(&cfg))
}

fn copilot_prompt(prompt_text: &str, model: Option<&str>) -> Result<String> {
    let cfg = load_config().unwrap_or_default();
    let copilot_bin = resolve_copilot_bin(&cfg);
    let mut cmd = Command::new(copilot_bin);
    cmd.arg("-p").arg(prompt_text).arg("-s");
    if let Some(m) = model {
        if !m.is_empty() {
            cmd.arg("--model").arg(m);
        }
    }

    let output = cmd.output().context("failed to run copilot CLI")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "copilot exited with status {:?}: {}",
            output.status.code(),
            stderr
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn print_block(label: &str, body: &str) {
    let colors = Colors::detect();
    let mut out = io::stdout();
    let indented: String = body
        .lines()
        .map(|line| format!("  {}", line))
        .collect::<Vec<_>>()
        .join("\n");

    let _ = writeln!(
        out,
        "{}{}{}\n{}\n",
        colors.bold_cyan, label, colors.reset, indented
    );
}

fn action_picker(suggestion: &str) -> Result<Action> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("enable raw mode")?;
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let result: Result<Action> = (|| {
        let mut idx = 0usize;
        let options = [Action::Execute, Action::Copy, Action::Explain, Action::Quit];

        // Show a bounded amount of text so the UI stays readable.
        let suggestion_lines: Vec<&str> = suggestion.lines().collect();
        let suggestion_preview = if suggestion_lines.len() > 12 {
            let mut s = suggestion_lines[..12].join("\n");
            s.push_str("\n…");
            s
        } else {
            suggestion.to_string()
        };

        let chosen = loop {
            terminal
                .draw(|f| {
                    let area = f.size().inner(&Margin {
                        horizontal: 2,
                        vertical: 1,
                    });

                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(2),
                            Constraint::Min(5),
                            Constraint::Length(options.len() as u16 + 2),
                        ])
                        .split(area);

                    let header = Paragraph::new(Line::from(vec![Span::styled(
                        "Action (↑/↓ + Enter, or hotkeys)",
                        Style::default().fg(Color::Gray),
                    )]));
                    f.render_widget(header, chunks[0]);

                    let suggestion_box = Paragraph::new(suggestion_preview.clone())
                        .block(Block::default().borders(Borders::ALL).title("Suggestion"))
                        .wrap(Wrap { trim: false });
                    f.render_widget(suggestion_box, chunks[1]);

                    let lines: Vec<Line> = options
                        .iter()
                        .enumerate()
                        .map(|(i, action)| {
                            let (label, hotkey) = match action {
                                Action::Execute => ("Execute command", "↵"),
                                Action::Copy => ("Copy to clipboard", "c"),
                                Action::Explain => ("Explain w/ Copilot", "x"),
                                Action::Quit => ("Quit", "q"),
                            };

                            let selected = i == idx;
                            let style = if selected {
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default()
                            };

                            let hotkey_style = Style::default().fg(Color::Gray);
                            if selected {
                                Line::from(vec![
                                    Span::raw(" > "),
                                    Span::styled(label, style),
                                    Span::raw("  "),
                                    Span::styled(format!("({hotkey})"), hotkey_style),
                                ])
                            } else {
                                Line::from(vec![
                                    Span::raw("   "),
                                    Span::styled(label, style),
                                    Span::raw("  "),
                                    Span::styled(format!("({hotkey})"), hotkey_style),
                                ])
                            }
                        })
                        .collect();

                    let list = Paragraph::new(lines).block(
                        Block::default()
                            .borders(Borders::NONE)
                            .padding(Padding::zero()),
                    );
                    f.render_widget(list, chunks[2]);
                })
                .context("draw TUI")?;

            if crossterm::event::poll(std::time::Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Up => {
                                if idx == 0 {
                                    idx = options.len() - 1;
                                } else {
                                    idx -= 1;
                                }
                            }
                            KeyCode::Down => {
                                idx = (idx + 1) % options.len();
                            }
                            KeyCode::Char('c') | KeyCode::Char('C') => break Action::Copy,
                            KeyCode::Char('x') | KeyCode::Char('X') => break Action::Explain,
                            KeyCode::Char('q') | KeyCode::Char('Q') => break Action::Quit,
                            KeyCode::Enter => break options[idx],
                            _ => {}
                        }
                    }
                }
            }
        };

        Ok(chosen)
    })();

    cleanup_terminal(&mut terminal)?;
    result
}

fn cleanup_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen")?;
    terminal.show_cursor().ok();
    Ok(())
}

fn confirm_execute(_cmd: &str) -> Result<bool> {
    let cfg = load_config().unwrap_or_default();
    if !resolve_confirm_execute(&cfg) {
        return Ok(true);
    }

    print!("Confirm execution: [y/N] ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim();
    Ok(trimmed.eq_ignore_ascii_case("y"))
}

fn run_shell(cmd: &str) -> Result<()> {
    let cfg = load_config().unwrap_or_default();
    let shell = resolve_shell(&cfg);

    let status = Command::new(shell)
        .arg("-c")
        .arg(cmd)
        .status()
        .context("failed to execute suggested command")?;

    if !status.success() {
        return Err(anyhow!("command exited with status {:?}", status.code()));
    }
    Ok(())
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text.to_string()).is_ok() {
            return Ok(());
        }
    }

    for tool in ["pbcopy", "wl-copy", "xclip"] {
        let mut cmd = Command::new(tool);
        if tool == "xclip" {
            cmd.arg("-selection").arg("clipboard");
        }

        let spawn = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match spawn {
            Ok(mut handle) => {
                if let Some(stdin) = handle.stdin.as_mut() {
                    stdin.write_all(text.as_bytes()).ok();
                }
                let status = handle.wait().ok();
                if matches!(status, Some(s) if s.success()) {
                    return Ok(());
                }
            }
            Err(_) => continue,
        }
    }

    Err(anyhow!("No clipboard tool found (pbcopy/wl-copy/xclip)"))
}

struct Colors {
    bold_cyan: &'static str,
    reset: &'static str,
}

impl Colors {
    fn detect() -> Self {
        if io::stdout().is_terminal() {
            Self {
                bold_cyan: "\u{1b}[1;36m",
                reset: "\u{1b}[0m",
            }
        } else {
            Self {
                bold_cyan: "",
                reset: "",
            }
        }
    }
}
