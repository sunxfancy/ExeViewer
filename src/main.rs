use std::io::{self, stdout};
use std::ops::Deref;
use std::path::PathBuf;
use clap::Parser;

use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode},
        terminal::{
            disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
        },
        ExecutableCommand,
    },
    widgets::{Block, Paragraph},
    Frame, Terminal,
};

use elf::ElfBytes;
use elf::endian::AnyEndian;
use elf::note::Note;
use elf::note::NoteGnuBuildId;
use elf::section::SectionHeader;


/// Simple program to greet a person
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path of the executable file
    file: PathBuf,
}


fn main() -> io::Result<()> {
    let args = Args::parse();

    let path = std::path::PathBuf::from(&args.file);
    let file_data = std::fs::read(path).expect("Could not read file.");
    let slice = file_data.as_slice();
    let file = ElfBytes::<AnyEndian>::minimal_parse(slice).expect("Open elf file");

    // Find lazy-parsing types for the common ELF sections (we want .dynsym, .dynstr, .hash)
    let symtable = file.symbol_table().expect("symtab should parse");
    let (symtab, strtab) = symtable.unwrap();

    let mut content = String::new();
    symtab.iter().for_each(|sym| {
        content.push_str( &format!("Symbol: {:?}\n", strtab.get(sym.st_name as usize).unwrap()) );
    });

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut should_quit = false;
    while !should_quit {
        terminal.draw(|frame: &mut Frame|{
            // ui(&mut frame);
            ui(frame, &args, &content);
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

fn ui(frame: &mut Frame, args: &Args, content: &String) {
    frame.render_widget(
        Paragraph::new(content.as_str()).block(Block::bordered().title("Greeting")),
        frame.area(),
    );
}