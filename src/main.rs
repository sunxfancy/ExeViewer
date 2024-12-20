use clap::Parser;
use ::elf::endian::AnyEndian;
use ::elf::ElfBytes;
use ratatui::buffer::Buffer;
use ratatui::style::palette::tailwind;
use ratatui::symbols;
use ratatui::text::Line;
use std::fs;
use std::io::{self, stdout};
use std::iter::Sum;
use std::path::PathBuf;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::Backend;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::widgets::{ListState, Padding, StatefulWidget, Tabs, Widget};
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
use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

mod elf;
mod summary;
mod symbol;
mod plt;

use summary::SummaryPage;
use symbol::SymbolPage;
use plt::PLTPage;

/// Simple program to greet a person
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path of the executable file
    file: PathBuf,
}

struct App<'a> {
    should_quit: bool,
    elf: ElfBytes::<'a, AnyEndian>,
    summary_page: SummaryPage<'a>,
    symbol_page: SymbolPage<'a>,
    plt_page: PLTPage<'a>,
    selected_tab: AppTab,
}

#[derive(Default, Clone, Copy, Display, FromRepr, EnumIter)]
enum AppTab {
    #[default]
    #[strum(to_string = "Summary")]
    Summary,
    #[strum(to_string = "Deassembly")]
    Deassembly,
    #[strum(to_string = "Dynamic Symbols & PLT")]
    PLT,
}

impl <'a> App<'a> {
    fn new(elf: ElfBytes::<'a, AnyEndian>) -> App<'a> {
        let (sectab,secstr) = elf
            .section_headers_with_strtab()
            .expect("sections should parse");

        // Find lazy-parsing types for the common ELF sections (we want .dynsym, .dynstr, .hash)
        let symtable = elf.symbol_table().expect("symtab should parse");
        let (symtab, strtab) = symtable.unwrap();

        // Find the dynamic symbol table and string table
        let dynsymtab = elf.dynamic_symbol_table().expect("dynsym should parse");
        let (dysymtab, dystrtab) = dynsymtab.unwrap();
        
        let rela_plt = elf.section_header_by_name(".rela.plt").expect("not found");
        let rela = elf.section_data_as_relas(&rela_plt.unwrap()).expect("rela should parse");

        let plt = elf.section_header_by_name(".plt").expect("not found").unwrap();

        App {
            should_quit: false,
            elf,
            summary_page: SummaryPage::new(sectab.expect("not found"), secstr.expect("not found")),
            symbol_page: SymbolPage::new(symtab, strtab),
            plt_page: PLTPage::new(rela, dysymtab, dystrtab, plt),
            selected_tab: AppTab::Summary,
        }
    }

    fn run<B: Backend>(mut self, mut terminal: Terminal<B>) -> Result<(), io::Error> {
        while !self.should_quit {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            self.should_quit = self.handle_events()?;
        }
        Ok(())
    }

    fn handle_events(&mut self) -> Result<bool, io::Error> {
        if event::poll(std::time::Duration::from_millis(20))? {
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
                        KeyCode::Right => {
                            self.select_right();
                        }
                        KeyCode::Left => {
                            self.select_left();
                        }
                        KeyCode::Char('1') => {
                            self.selected_tab = AppTab::Summary;
                        }
                        KeyCode::Char('2') => {
                            self.selected_tab = AppTab::Deassembly;
                        }
                        KeyCode::Char('3') => {
                            self.selected_tab = AppTab::PLT;
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(false)
    }

    fn select_next(&mut self) {
        match self.selected_tab {
            AppTab::Summary => self.summary_page.state.select_next(),
            AppTab::Deassembly => self.symbol_page.select_next(&self.elf),
            AppTab::PLT => self.plt_page.select_next(&self.elf),
        }
    }

    fn select_previous(&mut self) {
        match self.selected_tab {
            AppTab::Summary => self.summary_page.state.select_previous(),
            AppTab::Deassembly => self.symbol_page.select_previous(&self.elf),
            AppTab::PLT => self.plt_page.select_previous(&self.elf),
        }
    }

    fn select_left(&mut self) {
        match self.selected_tab {
            AppTab::Summary => {}
            AppTab::Deassembly => self.symbol_page.select_left(),
            AppTab::PLT => self.plt_page.select_left(),
        }
    }

    fn select_right(&mut self) {
        match self.selected_tab {
            AppTab::Summary => {}
            AppTab::Deassembly => self.symbol_page.select_right(),
            AppTab::PLT => self.plt_page.select_right(),
        }
    }

    fn render_tabs(&self, area: Rect, buf: &mut Buffer) {
        let titles = AppTab::iter().map(AppTab::title);
        let highlight_style = (Color::default(), self.selected_tab.palette().c700);
        let selected_tab_index = self.selected_tab as usize;
        Tabs::new(titles)
            .highlight_style(highlight_style)
            .select(selected_tab_index)
            .padding("", "")
            .divider(" ")
            .render(area, buf);
    }

    fn render_pages(&mut self, area: Rect, buf: &mut Buffer) {
        match self.selected_tab {
            AppTab::Summary => (&mut self.summary_page).render(area, buf),
            AppTab::Deassembly => (&mut self.symbol_page).render(area, buf),
            AppTab::PLT => (&mut self.plt_page).render(area, buf),
        }
    }
}

impl Widget for &mut App<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let vertical = Layout::vertical([Length(1), Min(0), Length(1)]);
        let [header_area, inner_area, footer_area] = vertical.areas(area);

        let horizontal = Layout::horizontal([Min(0), Length(20)]);
        let [tabs_area, title_area] = horizontal.areas(header_area);

        fn render_title(area: Rect, buf: &mut Buffer) {
            "Elf Viewer v1.0   ".bold().render(area, buf);
        }

        fn render_footer(area: Rect, buf: &mut Buffer) {
            Line::raw("1, 2, 3 select tabs |  ◄ ► to move between components | Press q to quit")
                .centered()
                .render(area, buf);
        }

        render_title(title_area, buf);
        self.render_tabs(tabs_area, buf);
        self.render_pages(inner_area, buf);
        render_footer(footer_area, buf);
    }
}

impl AppTab {
    /// Return tab's name as a styled `Line`
    fn title(self) -> Line<'static> {
        format!("  {self}  ")
            .fg(tailwind::SLATE.c200)
            .bg(self.palette().c900)
            .into()
    }

    /// A block surrounding the tab's content
    fn block(self) -> Block<'static> {
        Block::bordered()
            .border_set(symbols::border::PROPORTIONAL_TALL)
            .padding(Padding::horizontal(1))
            .border_style(self.palette().c700)
    }

    const fn palette(self) -> tailwind::Palette {
        match self {
            Self::Summary => tailwind::BLUE,
            Self::Deassembly => tailwind::EMERALD,
            Self::PLT => tailwind::INDIGO,
            // Self::Notes => tailwind::RED,
        }
    }
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let buffer = fs::read(&args.file).expect("file should read");
    let app = App::new(elf::parse(&buffer));

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    app.run(terminal)?;

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
