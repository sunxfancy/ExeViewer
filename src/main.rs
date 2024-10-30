use clap::Parser;
use std::fs;
use std::io::{self, stdout};
use std::path::PathBuf;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::Backend;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::ListState;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    widgets::{Block, List, ListDirection, Paragraph},
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

struct App<'a> {
    should_quit: bool,
    elf_file: Elf<'a>,
    symbol_page: SymbolPage<'a>,
}

trait Page {
    fn draw(&mut self, frame: &mut Frame) -> ();
}

struct SymbolPage<'a> {
    content: Vec<Symbol>,
    list: List<'a>,
    state: ListState,
}

struct Symbol {
    address: u64,
    size: u64,
    decompiled: bool,
    data: String,
}

impl App<'_> {
    fn new<'a>(elf_file: Elf<'a>, name_list: Vec<&'a str>, content: Vec<Symbol>) -> App<'a> {
        let list = List::new(name_list)
            .block(Block::bordered().title("Symbols"))
            .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::TopToBottom);
        App {
            should_quit: false,
            elf_file: elf_file,
            symbol_page: SymbolPage {
                content: content,
                list: list,
                state: ListState::default(),
            },
        }
    }

    fn run<B: Backend>(mut self, mut terminal: Terminal<B>) -> Result<(), io::Error> {
        while !self.should_quit {
            terminal.draw(|frame| {
                self.symbol_page.draw(frame);
            })?;
            self.should_quit = self.handle_events()?;
        }
        Ok(())
    }

    fn handle_events(&mut self) -> Result<bool, io::Error> {
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == event::KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(true),
                        KeyCode::Down => {
                            self.select_next();
                        }
                        KeyCode::Up => {
                            self.select_previous();
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(false)
    }

    fn load_symbol(&mut self, idx: usize) {
        let symbol = &self.symbol_page.content[idx];
        if !symbol.decompiled {
            let decompiled = self
                .elf_file
                .decompile_symbol(symbol.address, symbol.size as usize);
            self.symbol_page.content[idx].data = decompiled;
            self.symbol_page.content[idx].decompiled = true;
        }
    }

    fn select_next(&mut self) {
        self.symbol_page.state.select_next();
        let idx = self.symbol_page.state.selected().unwrap();
        self.load_symbol(idx);
    }

    fn select_previous(&mut self) {
        self.symbol_page.state.select_previous();
    }
}

impl Page for SymbolPage<'_> {
    fn draw(&mut self, frame: &mut Frame) -> () {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(frame.area());

        frame.render_stateful_widget(&self.list, layout[0], &mut self.state);

        let selected = self.state.selected();
        frame.render_widget(
            Paragraph::new(if selected.is_none() {
                "Select a symbol to decompile"
            } else {
                self.content[selected.unwrap()].data.as_str()
            })
            .block(Block::default().title("Assembly")),
            layout[1],
        )
    }
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let buffer = fs::read(&args.file).expect("file should read");
    let file: Elf<'_> = Elf::new(&buffer);

    // Find lazy-parsing types for the common ELF sections (we want .dynsym, .dynstr, .hash)
    let symtable = file.elf.symbol_table().expect("symtab should parse");
    let (symtab, strtab) = symtable.unwrap();

    let mut name_list: Vec<&str> = Vec::new();
    let mut content: Vec<Symbol> = Vec::new();
    symtab.iter().for_each(|sym| {
        let name = strtab.get(sym.st_name as usize).unwrap();
        name_list.push(name);
        content.push(Symbol {
            address: sym.st_value,
            size: sym.st_size,
            decompiled: false,
            data: String::new(),
        });
    });

    let app = App::new(file, name_list, content);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    app.run(terminal)?;

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
