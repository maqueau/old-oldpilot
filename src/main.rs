use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const CS_PREFIX: &str = "[[CPLT:CS]]";
const CE_PREFIX: &str = "[[CPLT:CE]]";
const DEFAULT_MODEL: &str = "gpt-4.1";
const COPILOT_INSTALL_HINT: &str = "npm install -g @github/copilot";

#[derive(Parser)]
#[command(name = "oldpilot", version, about = "Copilot CLI helper restoring explain and suggest workflows")]
struct Cli { #[command(subcommand)] command: Commands }

#[derive(Subcommand)]
enum Commands {
    Ce { prompt: Vec<String> }, Cex { prompt: Vec<String> },
    Cs { prompt: Vec<String> }, Csx { prompt: Vec<String> },
    Settings, Doctor,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Config { default_model: Option<String>, confirm_execute: Option<bool>, copilot_bin: Option<String>, shell: Option<String> }

#[derive(Debug, PartialEq, Eq)]
struct DoctorReport { copilot: Option<PathBuf>, clipboard: bool, terminal: bool, shell: Option<PathBuf> }

fn config_path() -> Result<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))).context("HOME or XDG_CONFIG_HOME is not set")?;
    Ok(base.join("oldpilot").join("config"))
}

fn load_config() -> Result<Config> {
    let path = config_path()?;
    let text = match fs::read_to_string(path) { Ok(s) => s, Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()), Err(e) => return Err(e.into()) };
    let mut c = Config::default();
    for raw in text.lines() {
        let line = raw.trim(); if line.is_empty() || line.starts_with('#') { continue; }
        let Some((key, value)) = line.split_once('=') else { continue };
        let mut value = value.trim().to_string();
        if value.len() >= 2 && ((value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\''))) { value = value[1..value.len()-1].to_string(); }
        match key.trim() {
            "default_model" if !value.is_empty() => c.default_model = Some(value),
            "copilot_bin" if !value.is_empty() => c.copilot_bin = Some(value),
            "shell" if !value.is_empty() => c.shell = Some(value),
            "confirm_execute" => c.confirm_execute = match value.to_ascii_lowercase().as_str() { "true"|"1"|"yes" => Some(true), "false"|"0"|"no" => Some(false), _ => c.confirm_execute },
            _ => {}
        }
    }
    Ok(c)
}

fn save_config(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(dir) = path.parent() { fs::create_dir_all(dir).context("failed to create config directory")?; }
    let mut out = String::new();
    out.push_str("# oldpilot config\n# Lines are key=value. Strings may be quoted.\n\n");
    if let Some(m) = &cfg.default_model { out.push_str(&format!("default_model=\"{}\"\n", m.replace('"', "\\\""))); }
    if let Some(b) = cfg.confirm_execute { out.push_str(&format!("confirm_execute={}\n", if b { "true" } else { "false" })); }
    if let Some(b) = &cfg.copilot_bin { out.push_str(&format!("copilot_bin=\"{}\"\n", b.replace('"', "\\\""))); }
    if let Some(s) = &cfg.shell { out.push_str(&format!("shell=\"{}\"\n", s.replace('"', "\\\""))); }
    fs::write(&path, out).context("failed to write config")
}

fn env_or_config(env_name: &str, value: Option<String>, fallback: &str) -> String { env::var(env_name).ok().filter(|v| !v.trim().is_empty()).or(value).unwrap_or_else(|| fallback.to_string()) }
fn default_model(c: &Config) -> String { env_or_config("COPILOT_DEFAULT_MODEL", c.default_model.clone(), DEFAULT_MODEL) }
fn copilot_name(c: &Config) -> String { env_or_config("COPILOT_BIN", c.copilot_bin.clone(), "copilot") }
fn shell_name(c: &Config) -> String { env_or_config("COPILOT_SHELL", c.shell.clone(), "sh") }
fn confirm_enabled(c: &Config) -> bool { match env::var("COPILOT_CS_CONFIRM_EXECUTE").ok().as_deref() { Some("false"|"0"|"no"|"NO") => false, Some("true"|"1"|"yes"|"YES") => true, _ => c.confirm_execute.unwrap_or(true) } }

fn cfg_string_display(v: &Option<String>, fallback: &str) -> String { v.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(fallback).to_string() }
fn cfg_bool_display(v: Option<bool>, fallback: bool) -> String { match v { Some(true) => "true".to_string(), Some(false) => "false".to_string(), None => if fallback { "true" } else { "false" }.to_string() } }

fn executable(path_or_name: &str) -> Option<PathBuf> {
    let path = Path::new(path_or_name);
    if path.components().count() > 1 || path.is_absolute() { return path.is_file().then(|| path.to_path_buf()); }
    env::var_os("PATH").and_then(|paths| env::split_paths(&paths).map(|p| p.join(path_or_name)).find(|p| p.is_file()))
}

fn resolve_copilot(c: &Config) -> Result<PathBuf> {
    executable(&copilot_name(c)).ok_or_else(|| anyhow!("GitHub Copilot CLI was not found. Install it with `{}` and verify it with `command -v copilot`.", COPILOT_INSTALL_HINT))
}
fn resolve_shell(c: &Config) -> Option<PathBuf> { executable(&shell_name(c)) }

fn clipboard_available() -> bool {
    arboard::Clipboard::new().is_ok() || ["pbcopy", "wl-copy", "xclip"].iter().any(|x| executable(x).is_some())
}

fn doctor_report(c: &Config) -> DoctorReport { DoctorReport { copilot: resolve_copilot(c).ok(), clipboard: clipboard_available(), terminal: io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal(), shell: resolve_shell(c) } }
fn run_doctor() -> Result<()> {
    let c = load_config().unwrap_or_default(); let r = doctor_report(&c);
    println!("oldpilot doctor");
    println!("  copilot: {}{}", if r.copilot.is_some() { "OK" } else { "MISSING" }, r.copilot.as_ref().map(|p| format!(" ({})", p.display())).unwrap_or_default());
    println!("  clipboard: {}", if r.clipboard { "OK" } else { "MISSING" });
    println!("  shell: {}{}", if r.shell.is_some() { "OK" } else { "MISSING" }, r.shell.as_ref().map(|p| format!(" ({})", p.display())).unwrap_or_default());
    println!("  terminal: {}", if r.terminal { "interactive" } else { "non-interactive" });
    if r.copilot.is_none() { println!("  fix: install Copilot CLI with `{}`", COPILOT_INSTALL_HINT); }
    if !r.clipboard { println!("  fix: install clipboard support or pbcopy/wl-copy/xclip"); }
    if r.shell.is_none() { println!("  fix: install the configured shell `{}` or set COPILOT_SHELL", shell_name(&c)); }
    if !r.terminal { println!("  fix: run interactive commands from a terminal"); }
    if r.copilot.is_none() || r.shell.is_none() || !r.clipboard || !r.terminal { return Err(anyhow!("doctor found one or more unmet requirements")); }
    Ok(())
}

fn copilot_prompt(prompt: &str, model: Option<&str>) -> Result<String> {
    let c = load_config().unwrap_or_default(); let mut cmd = Command::new(resolve_copilot(&c)?);
    cmd.args(["-p", prompt, "-s"]); if let Some(model) = model.filter(|m| !m.is_empty()) { cmd.args(["--model", model]); }
    let out = cmd.output().context("failed to run Copilot CLI")?;
    if !out.status.success() { return Err(anyhow!("copilot exited with status {:?}: {}", out.status.code(), String::from_utf8_lossy(&out.stderr).trim())); }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn print_block(label: &str, text: &str) { println!("\x1b[1;36m{}\x1b[0m\n{}\n", label, text.lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n")); }
fn run_ce(parts: Vec<String>, model: Option<&str>) -> Result<()> { let p = parts.join(" "); if p.trim().is_empty() { return Err(anyhow!("Prompt cannot be empty")); } print!("{}", copilot_prompt(&format!("{} {}", CE_PREFIX, p), model)?); Ok(()) }

fn action_picker(suggestion: &str) -> Result<char> {
    let mut out = io::stdout(); enable_raw_mode()?; execute!(out, EnterAlternateScreen)?; let mut terminal = Terminal::new(CrosstermBackend::new(out))?;
    let result = (|| -> Result<char> { loop {
        terminal.draw(|f| { let area = f.size().inner(&Margin { horizontal: 2, vertical: 1 }); let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(5), Constraint::Length(1)]).split(area); f.render_widget(Paragraph::new(suggestion).block(Block::default().borders(Borders::ALL).title("Suggestion")).wrap(Wrap { trim: false }), chunks[0]); f.render_widget(Paragraph::new("[Enter] execute  [c] copy  [x] explain  [q] quit"), chunks[1]); })?;
        if event::poll(Duration::from_millis(250))? { if let Event::Key(k) = event::read()? { if k.kind != KeyEventKind::Press { continue } match k.code { KeyCode::Enter => break Ok('e'), KeyCode::Char('c') => break Ok('c'), KeyCode::Char('x') => break Ok('x'), KeyCode::Char('q') | KeyCode::Esc => break Ok('q'), _ => {} } } }
    }})();
    disable_raw_mode().ok(); execute!(terminal.backend_mut(), LeaveAlternateScreen).ok(); terminal.show_cursor().ok(); result
}

fn confirm_execute(c: &Config) -> Result<bool> {
    if !confirm_enabled(c) { return Ok(true); }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() { return Ok(false); }
    print!("Confirm execution? [y/N] "); io::stdout().flush()?; enable_raw_mode()?;
    let result = (|| -> Result<bool> { loop { if let Event::Key(k) = event::read()? { if k.kind == KeyEventKind::Press { break Ok(matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y'))); } } } })();
    disable_raw_mode().ok(); println!(); result
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    if let Ok(mut cb) = arboard::Clipboard::new() { if cb.set_text(text.to_string()).is_ok() { return Ok(()) } }
    for tool in ["pbcopy", "wl-copy", "xclip"] { let mut cmd = Command::new(tool); if tool == "xclip" { cmd.args(["-selection", "clipboard"]); } if let Ok(mut child) = cmd.stdin(Stdio::piped()).spawn() { if let Some(stdin) = child.stdin.as_mut() { let _ = stdin.write_all(text.as_bytes()); } if child.wait().map(|s| s.success()).unwrap_or(false) { return Ok(()) } } }
    Err(anyhow!("No usable clipboard provider found"))
}

fn run_cs(parts: Vec<String>, model: Option<&str>) -> Result<()> {
    let p = parts.join(" "); if p.trim().is_empty() { return Err(anyhow!("Prompt cannot be empty")); }
    let suggestion = copilot_prompt(&format!("{} TASK: {}", CS_PREFIX, p), model)?.trim().to_string(); if suggestion.is_empty() { println!("No suggestion returned."); return Ok(()) }
    print_block("Copilot", &suggestion); let c = load_config().unwrap_or_default();
    match action_picker(&suggestion)? {
        'e' => if confirm_execute(&c)? { let shell = resolve_shell(&c).ok_or_else(|| anyhow!("Configured shell `{}` was not found", shell_name(&c)))?; let status = Command::new(shell).arg("-c").arg(&suggestion).status()?; if !status.success() { return Err(anyhow!("command exited with status {:?}", status.code())); } } else { println!("Canceled."); },
        'c' => { copy_to_clipboard(&suggestion)?; println!("Copied to clipboard."); },
        'x' => run_ce(vec![format!("Explain this command: {}", suggestion)], model)?,
        _ => println!("Canceled."),
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsField { DefaultModel, ConfirmExecute, CopilotBin, Shell, Save, Quit }

const SETTINGS_FIELDS: [SettingsField; 6] = [SettingsField::DefaultModel, SettingsField::ConfirmExecute, SettingsField::CopilotBin, SettingsField::Shell, SettingsField::Save, SettingsField::Quit];

fn run_settings() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() { println!("Interactive settings requires a terminal. Config file: {}", config_path()?.display()); return Ok(()); }
    let mut cfg = load_config().unwrap_or_default();
    let mut out = io::stdout(); enable_raw_mode()?; execute!(out, EnterAlternateScreen)?; let mut terminal = Terminal::new(CrosstermBackend::new(out))?;

    let result: Result<()> = (|| {
        let mut idx = 0usize;
        let mut editing: Option<SettingsField> = None;
        let mut edit_buf = String::new();
        let mut status = String::from("↑/↓ move · Enter edit/toggle/save · Esc cancel edit · q quit");

        loop {
            terminal.draw(|f| {
                let area = f.size().inner(&Margin { horizontal: 2, vertical: 1 });
                let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(2), Constraint::Min(6), Constraint::Length(2)]).split(area);
                f.render_widget(Paragraph::new(Line::from(vec![Span::styled("oldpilot settings", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))])), chunks[0]);

                let effective_model = default_model(&cfg);
                let effective_confirm = confirm_enabled(&cfg);
                let effective_bin = copilot_name(&cfg);
                let effective_shell = shell_name(&cfg);

                let lines: Vec<Line> = SETTINGS_FIELDS.iter().enumerate().map(|(i, field)| {
                    let selected = i == idx;
                    let base_style = if selected { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default() };
                    let (label, value) = match field {
                        SettingsField::DefaultModel => ("Default model", cfg_string_display(&cfg.default_model, &effective_model)),
                        SettingsField::ConfirmExecute => ("Confirm execute", cfg_bool_display(cfg.confirm_execute, effective_confirm)),
                        SettingsField::CopilotBin => ("Copilot binary", cfg_string_display(&cfg.copilot_bin, &effective_bin)),
                        SettingsField::Shell => ("Shell", cfg_string_display(&cfg.shell, &effective_shell)),
                        SettingsField::Save => ("Save", String::new()),
                        SettingsField::Quit => ("Quit", String::new()),
                    };
                    let is_editing = editing == Some(*field);
                    let shown = if is_editing { format!("{}_", edit_buf) } else { value };
                    Line::from(vec![Span::styled(format!("{:<16}", label), base_style), Span::raw(shown)])
                }).collect();
                f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Fields")), chunks[1]);
                f.render_widget(Paragraph::new(status.clone()), chunks[2]);
            })?;

            if !event::poll(Duration::from_millis(250))? { continue; }
            let Event::Key(k) = event::read()? else { continue };
            if k.kind != KeyEventKind::Press { continue; }

            if let Some(field) = editing {
                match k.code {
                    KeyCode::Enter => {
                        let value = edit_buf.trim().to_string();
                        let value = if value.is_empty() { None } else { Some(value) };
                        match field {
                            SettingsField::DefaultModel => cfg.default_model = value,
                            SettingsField::CopilotBin => cfg.copilot_bin = value,
                            SettingsField::Shell => cfg.shell = value,
                            _ => {}
                        }
                        editing = None; edit_buf.clear(); status = "Updated (not saved yet — select Save to persist).".to_string();
                    }
                    KeyCode::Esc => { editing = None; edit_buf.clear(); status = "Edit canceled.".to_string(); }
                    KeyCode::Backspace => { edit_buf.pop(); }
                    KeyCode::Char(c) => edit_buf.push(c),
                    _ => {}
                }
                continue;
            }

            match k.code {
                KeyCode::Up => idx = idx.checked_sub(1).unwrap_or(SETTINGS_FIELDS.len() - 1),
                KeyCode::Down => idx = (idx + 1) % SETTINGS_FIELDS.len(),
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Enter => match SETTINGS_FIELDS[idx] {
                    SettingsField::ConfirmExecute => { cfg.confirm_execute = Some(!confirm_enabled(&cfg)); status = "Toggled (not saved yet — select Save to persist).".to_string(); }
                    SettingsField::Save => match save_config(&cfg) { Ok(()) => status = format!("Saved to {}", config_path()?.display()), Err(e) => status = format!("Save failed: {}", e) },
                    SettingsField::Quit => break,
                    field @ (SettingsField::DefaultModel | SettingsField::CopilotBin | SettingsField::Shell) => {
                        editing = Some(field);
                        edit_buf = match field {
                            SettingsField::DefaultModel => cfg.default_model.clone(),
                            SettingsField::CopilotBin => cfg.copilot_bin.clone(),
                            SettingsField::Shell => cfg.shell.clone(),
                            _ => None,
                        }.unwrap_or_default();
                        status = "Editing — Enter to confirm, Esc to cancel.".to_string();
                    }
                },
                _ => {}
            }
        }
        Ok(())
    })();

    disable_raw_mode().ok(); execute!(terminal.backend_mut(), LeaveAlternateScreen).ok(); terminal.show_cursor().ok();
    result
}

fn main() -> Result<()> { let cli = Cli::parse(); match cli.command {
    Commands::Ce { prompt } => { let c = load_config().unwrap_or_default(); let m = default_model(&c); run_ce(prompt, Some(&m)) }
    Commands::Cex { prompt } => run_ce(prompt, None),
    Commands::Cs { prompt } => { let c = load_config().unwrap_or_default(); let m = default_model(&c); run_cs(prompt, Some(&m)) }
    Commands::Csx { prompt } => run_cs(prompt, None),
    Commands::Doctor => run_doctor(),
    Commands::Settings => run_settings(),
} }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn env_precedes_config() { let c = Config { default_model: Some("config-model".into()), ..Default::default() }; std::env::set_var("COPILOT_DEFAULT_MODEL", "env-model"); assert_eq!(default_model(&c), "env-model"); std::env::remove_var("COPILOT_DEFAULT_MODEL"); }
    #[test] fn shell_lookup_is_deterministic() { assert!(executable("sh").is_some()); }
    #[test] fn missing_binary_is_reported() { let c = Config { copilot_bin: Some("definitely-not-a-real-oldpilot-binary".into()), ..Default::default() }; assert!(resolve_copilot(&c).is_err()); }
    #[test] fn cfg_string_display_uses_fallback_when_empty() { assert_eq!(cfg_string_display(&None, "fallback"), "fallback"); assert_eq!(cfg_string_display(&Some("set".into()), "fallback"), "set"); }
    #[test] fn cfg_bool_display_reflects_override_or_fallback() { assert_eq!(cfg_bool_display(None, true), "true"); assert_eq!(cfg_bool_display(Some(false), true), "false"); }
}
