use std::fs;
use std::io::{self, stdout};
use std::path::PathBuf;
use clap::Parser;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::ListState;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode},
        terminal::{
            disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
        },
        ExecutableCommand,
    },
    widgets::{Block, Paragraph, List, ListDirection},
    Frame, Terminal,
};

mod elf;
use elf::Elf;

/// Simple program to greet a person
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path of the executable file
    file: PathBuf,
}


fn main() -> io::Result<()> {
    let args = Args::parse();
    let buffer = fs::read(&args.file).expect("file should read");
    let file = Elf::new(&buffer);
    
    // Find lazy-parsing types for the common ELF sections (we want .dynsym, .dynstr, .hash)
    let symtable = file.elf.symbol_table().expect("symtab should parse");
    let (symtab, strtab) = symtable.unwrap();

    let mut content = Vec::new();
    symtab.iter().for_each(|sym| {
        content.push(strtab.get(sym.st_name as usize).unwrap());
    });

    let list = List::new(content)
        .block(Block::bordered().title("Symbols"))
        .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
        .highlight_symbol(">>")
        .repeat_highlight_symbol(true)
        .direction(ListDirection::TopToBottom);
    let mut state = ListState::default();

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut should_quit = false;
    while !should_quit {
        terminal.draw(|frame: &mut Frame|{
            // ui(&mut frame);
            ui(frame, &args, &list, &mut state);
        })?;
        should_quit = handle_events()?;
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn handle_events() -> io::Result<bool> {
    if event::poll(std::time::Duration::from_millis(50))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == event::KeyEventKind::Press && key.code == KeyCode::Char('q') {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn ui(frame: &mut Frame, args: &Args, list: &List, state: &mut ListState) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Percentage(30),
            Constraint::Percentage(70),
        ])
        .split(frame.area());

    frame.render_stateful_widget(
        list,
        layout[0],
        state,
    );

    frame.render_widget(
        Paragraph::new(args.file.to_str().unwrap()).block(Block::default().title("File")),
        layout[1],
    )
}