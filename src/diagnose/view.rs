//! The interactive frontend consumes the exact same report as text/JSON.
use super::Report;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode, KeyEventKind},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Paragraph, Tabs},
    Terminal,
};
use std::io::{self, IsTerminal};

struct RestoreTerminal;
impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
    }
}

pub fn show(report: &Report) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "diagnose --interactive requires a terminal; omit it for text or use --format json"
                .into(),
        );
    }
    let pages = [
        format!("Status: {}\nArtifact: {:?}\nTarget root: {:?}\n\nChecked scope:\n{}\n\nLimits:\n{}\n\nObjects: {}\nDependencies: {}\nFindings: {}", report.status, report.artifact, report.environment.rootfs, report.scope.join("\n"), report.limitations.join("\n"), report.objects.len(), report.dependencies.len(), report.diagnostics.len()),
        serde_json::to_string_pretty(&report.diagnostics).map_err(|e|e.to_string())?,
        report.text().split("Dependency search evidence:\n").nth(1).unwrap_or("").to_string(),
        serde_json::to_string_pretty(&report.environment).map_err(|e|e.to_string())?,
    ];
    enable_raw_mode().map_err(|e| e.to_string())?;
    let _restore = RestoreTerminal;
    io::stdout()
        .execute(EnterAlternateScreen)
        .map_err(|e| e.to_string())?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|e| e.to_string())?;
    let mut tab = 0usize;
    let mut row = 0u16;
    let mut column = 0u16;
    loop {
        terminal
            .draw(|frame| {
                let areas = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(frame.area());
                frame.render_widget(
                    Tabs::new([
                        "1 Summary",
                        "2 Findings",
                        "3 Search evidence",
                        "4 Environment",
                    ])
                    .select(tab)
                    .highlight_style(Style::default().fg(Color::Cyan)),
                    areas[0],
                );
                frame.render_widget(
                    Paragraph::new(pages[tab].as_str())
                        .scroll((row, column))
                        .block(Block::bordered().title("ELF compatibility diagnosis")),
                    areas[1],
                );
                frame.render_widget(
                    Paragraph::new(
                        "1–4: page | ↑↓/PgUp/PgDn: scroll | ←→: horizontal | Home: reset | q: quit",
                    ),
                    areas[2],
                );
            })
            .map_err(|e| e.to_string())?;
        if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char(c @ '1'..='4') => {
                    tab = c as usize - '1' as usize;
                    row = 0;
                    column = 0;
                }
                KeyCode::Down => row = row.saturating_add(1),
                KeyCode::Up => row = row.saturating_sub(1),
                KeyCode::PageDown => row = row.saturating_add(20),
                KeyCode::PageUp => row = row.saturating_sub(20),
                KeyCode::Right => column = column.saturating_add(4),
                KeyCode::Left => column = column.saturating_sub(4),
                KeyCode::Home => {
                    row = 0;
                    column = 0;
                }
                _ => {}
            }
            row = row.min(
                pages[tab]
                    .lines()
                    .count()
                    .saturating_sub(1)
                    .min(u16::MAX as usize) as u16,
            );
        }
    }
    Ok(())
}
