use ::elf::endian::AnyEndian;
use ::elf::ElfBytes;
use clap::Parser;
use ratatui::buffer::Buffer;
use ratatui::style::palette::tailwind;
use ratatui::symbols;
use ratatui::text::Line;
use sha2::{Digest, Sha256};
use std::env;
use std::io::{self, stdout};
use std::path::PathBuf;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Backend;
use ratatui::style::{Color, Stylize};
use ratatui::widgets::{Padding, Tabs, Widget};
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    widgets::Block,
    Terminal,
};
use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

mod deps;
mod elf;
mod empty;
mod plt;
mod section;
mod summary;
mod symbol;
mod utils;

use deps::DependenciesPage;
use empty::{EmptyPage, Page};
use plt::PLTPage;
use section::SectionPage;
use summary::SummaryPage;
use symbol::SymbolPage;

/// Simple program to greet a person
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path of the executable file
    file: PathBuf,
}

struct App<'a> {
    should_quit: bool,
    elf: ElfBytes<'a, AnyEndian>,
    summary_page: SummaryPage,
    section_page: SectionPage<'a>,
    symbol_page: Box<dyn Page<'a> + 'a>,
    plt_page: PLTPage<'a>,
    deps_page: DependenciesPage<'a>,
    selected_tab: AppTab,
}

#[derive(Default, Clone, Copy, Display, FromRepr, EnumIter)]
enum AppTab {
    #[default]
    #[strum(to_string = "Summary")]
    Summary,
    #[strum(to_string = "Sections")]
    Sections,
    #[strum(to_string = "Deassembly")]
    Deassembly,
    #[strum(to_string = "Dynamic Symbols & PLT")]
    PLT,
    #[strum(to_string = "Dependencies")]
    Dependencies,
}

impl<'a> App<'a> {
    fn new(path: &PathBuf, file_hash: String, elf: ElfBytes<'a, AnyEndian>) -> App<'a> {
        let metadata = std::fs::metadata(path).expect("Failed to get file metadata");

        // Get compiler info from .comment section
        let compiler_info = elf
            .section_header_by_name(".comment")
            .ok()
            .flatten()
            .and_then(|header| elf.section_data(&header).ok())
            .and_then(|(data, _)| String::from_utf8(data.to_vec()).ok());

        let (sectab, secstr) = elf
            .section_headers_with_strtab()
            .expect("sections should parse");

        // Find lazy-parsing types for the common ELF sections (we want .dynsym, .dynstr, .hash)
        let symtable = elf.symbol_table().expect("symtab should parse");
        let symbol_page: Box<dyn Page + 'a> = if let Some((symtab, strtab)) = symtable {
            Box::new(SymbolPage::new(symtab, strtab))
        } else {
            Box::new(EmptyPage::new())
        };

        // Find the dynamic symbol table and string table
        let dynsymtab = elf.dynamic_symbol_table().expect("dynsym should parse");
        let (dysymtab, dystrtab) = dynsymtab.unwrap();

        let rela_plt = elf.section_header_by_name(".rela.plt").expect("not found");
        let rela = elf
            .section_data_as_relas(&rela_plt.unwrap())
            .expect("rela should parse");

        let plt = elf
            .section_header_by_name(".plt")
            .expect("not found")
            .unwrap();

        let dynamic = elf.dynamic().ok().flatten();
        let elf_header = elf.ehdr.clone();
        let interpreter = elf::get_interpreter(&elf);

        App {
            should_quit: false,
            elf,
            summary_page: SummaryPage::new(
                path.clone(),
                metadata,
                file_hash,
                elf_header,
                compiler_info,
                interpreter.clone(),
            ),
            section_page: SectionPage::new(sectab.expect("not found"), secstr.expect("not found")),
            symbol_page,
            plt_page: PLTPage::new(rela, dysymtab, dystrtab, plt),
            deps_page: DependenciesPage::new(
                dynamic,
                Some(dystrtab),
                interpreter.as_deref(),
                path.to_str().unwrap_or(""),
            ),
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
                            self.selected_tab = AppTab::Sections;
                        }
                        KeyCode::Char('3') => {
                            self.selected_tab = AppTab::Deassembly;
                        }
                        KeyCode::Char('4') => {
                            self.selected_tab = AppTab::PLT;
                        }
                        KeyCode::Char('5') => {
                            self.selected_tab = AppTab::Dependencies;
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
            AppTab::Summary => {}
            AppTab::Sections => self.section_page.state.select_next(),
            AppTab::Deassembly => self.symbol_page.select_next(&self.elf),
            AppTab::PLT => self.plt_page.select_next(&self.elf),
            AppTab::Dependencies => self.deps_page.state.select_next(),
        }
    }

    fn select_previous(&mut self) {
        match self.selected_tab {
            AppTab::Summary => {}
            AppTab::Sections => self.section_page.state.select_previous(),
            AppTab::Deassembly => self.symbol_page.select_previous(&self.elf),
            AppTab::PLT => self.plt_page.select_previous(&self.elf),
            AppTab::Dependencies => self.deps_page.state.select_previous(),
        }
    }

    fn select_left(&mut self) {
        match self.selected_tab {
            AppTab::Summary => {}
            AppTab::Sections => {}
            AppTab::Deassembly => self.symbol_page.select_left(),
            AppTab::PLT => self.plt_page.select_left(),
            AppTab::Dependencies => {}
        }
    }

    fn select_right(&mut self) {
        match self.selected_tab {
            AppTab::Summary => {}
            AppTab::Sections => {}
            AppTab::Deassembly => self.symbol_page.select_right(),
            AppTab::PLT => self.plt_page.select_right(),
            AppTab::Dependencies => {}
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
            AppTab::Summary => (&self.summary_page).render(area, buf),
            AppTab::Sections => (&mut self.section_page).render(area, buf),
            AppTab::Deassembly => (&mut self.symbol_page).page_render(area, buf),
            AppTab::PLT => (&mut self.plt_page).render(area, buf),
            AppTab::Dependencies => (&mut self.deps_page).render(area, buf),
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
            Line::raw("1, 2, 3, 4 select tabs |  ◄ ► to move between components | Press q to quit")
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
            Self::Sections => tailwind::EMERALD,
            Self::Deassembly => tailwind::INDIGO,
            Self::PLT => tailwind::AMBER,
            Self::Dependencies => tailwind::PURPLE,
        }
    }
}

fn main() -> io::Result<()> {
    if env::var("RUST_LOG").is_ok() {
    let _ = simple_logging::log_to_file("exeviewer.log", log::LevelFilter::Info);
    }

    let args = Args::parse();
    let (file_path, buffer) = utils::find_executable(&args.file)?;

    let file_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&buffer);
        format!("{:X}", hasher.finalize())
    };

    let app = App::new(&file_path, file_hash, elf::parse(&buffer));

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    app.run(terminal)?;

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
