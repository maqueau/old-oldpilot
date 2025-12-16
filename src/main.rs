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
use ratatui::widgets::{Block, Borders, Padding, Paragraph};
use ratatui::Terminal;
use std::env;
use std::io::{self, IsTerminal, Write};
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Execute,
    Copy,
    Explain,
    Quit,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Ce { prompt } => run_ce(prompt, Some(default_model()?.as_str())),
        Commands::Cex { prompt } => run_ce(prompt, None),
        Commands::Cs { prompt } => run_cs(prompt, Some(default_model()?.as_str())),
        Commands::Csx { prompt } => run_cs(prompt, None),
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

    let action = action_picker()?;
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
    Ok(env::var("COPILOT_DEFAULT_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string()))
}

fn copilot_prompt(prompt_text: &str, model: Option<&str>) -> Result<String> {
    let mut cmd = Command::new("copilot");
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

fn action_picker() -> Result<Action> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("enable raw mode")?;
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let result: Result<Action> = (|| {
        let mut idx = 0usize;
        let options = [Action::Execute, Action::Copy, Action::Explain, Action::Quit];

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
                            Constraint::Length(3),
                            Constraint::Length(options.len() as u16 + 2),
                        ])
                        .split(area);

                    let header = Paragraph::new(Line::from(vec![Span::styled(
                        "Action (↑/↓ + Enter, or hotkeys)",
                        Style::default().fg(Color::Gray),
                    )]));
                    f.render_widget(header, chunks[0]);

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
                    f.render_widget(list, chunks[1]);
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
    let confirm_flag =
        env::var("COPILOT_CS_CONFIRM_EXECUTE").unwrap_or_else(|_| "true".to_string());
    if confirm_flag.eq_ignore_ascii_case("false") {
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
    let status = Command::new("sh")
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
