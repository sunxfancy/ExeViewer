use elf::{
    dynamic::Dyn, endian::AnyEndian, parse::ParsingTable, string_table::StringTable, abi
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListDirection, ListState, Paragraph, StatefulWidget, Widget},
};

pub struct DependenciesPage<'a> {
    pub rpath: Option<String>,
    pub needed: Vec<DependencyEntry<'a>>,
    pub list: List<'a>,
    pub state: ListState,
}

pub struct DependencyEntry<'a> {
    pub name: &'a str,
    pub is_critical: bool,
    pub search_path: String,
}

impl<'a> DependenciesPage<'a> {
    pub fn new(
        dynamic: Option<ParsingTable<'a, AnyEndian, Dyn>>,
        dynstr: Option<StringTable<'a>>,
    ) -> DependenciesPage<'a> {
        let mut rpath = None;
        let mut needed = Vec::new();

        // Get dynamic section
        if let Some(dynamic) = dynamic {
            // Extract RPATH
            if let Some(rpath_entry) = dynamic.iter().find(|d| d.d_tag == abi::DT_RPATH) {
                if let Some(dynstr) = &dynstr {
                    if let Ok(path) = dynstr.get(rpath_entry.d_val() as usize) {
                        rpath = Some(path.to_string());
                    }
                }
            }

            // Extract needed libraries
            if let Some(dynstr) = &dynstr {
                for entry in dynamic.iter() {
                    if entry.d_tag == abi::DT_NEEDED {
                        if let Ok(name) = dynstr.get(entry.d_val() as usize) {
                            let is_critical = Self::is_critical_library(name);
                            needed.push(DependencyEntry {
                                name,
                                is_critical,
                                search_path: Self::get_search_path(name, rpath.as_deref()),
                            });
                        }
                    }
                }
            }
        }

        let list_items = needed.iter().map(|entry| {
            if entry.is_critical {
                format!("* {}", entry.name)
            } else {
                entry.name.to_string()
            }
        }).collect::<Vec<_>>();

        let list = List::new(list_items)
            .block(Block::bordered().title("Dependencies"))
            .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
            .highlight_symbol(">> ")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::TopToBottom);

        DependenciesPage {
            rpath,
            needed,
            list,
            state: ListState::default(),
        }
    }

    fn is_critical_library(name: &str) -> bool {
        let critical_libs = [
            "libc.so",
            "libstdc++.so",
            "libgcc_s.so",
            "ld-linux",
        ];
        critical_libs.iter().any(|lib| name.starts_with(lib))
    }

    fn get_search_path(name: &str, rpath: Option<&str>) -> String {
        let mut paths = Vec::new();
        
        // 1. RPATH/RUNPATH
        if let Some(rpath) = rpath {
            paths.push(rpath.to_string());
        }
        
        // 2. LD_LIBRARY_PATH (environment variable)
        if let Ok(ld_path) = std::env::var("LD_LIBRARY_PATH") {
            paths.push(ld_path);
        }
        
        // 3. Default system paths
        paths.extend_from_slice(&[
            "/lib".to_string(),
            "/usr/lib".to_string(),
            "/lib64".to_string(),
            "/usr/lib64".to_string(),
        ]);

        paths.join(":")
    }
}

impl Widget for &mut DependenciesPage<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(40), Constraint::Percentage(100)])
            .split(area);

        StatefulWidget::render(&self.list, layout[0], buf, &mut self.state);

        let details = if let Some(selected) = self.state.selected() {
            let entry = &self.needed[selected];
            vec![
                Line::from(vec![
                    Span::raw("Library: "),
                    Span::styled(entry.name, Style::default().add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Type: "),
                    Span::styled(
                        if entry.is_critical { "Critical System Library" } else { "Regular Library" },
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from("Search Path:"),
                Line::from(entry.search_path.clone()),
            ]
        } else {
            let mut lines = vec![
                Line::from("RPATH:"),
                Line::from(self.rpath.as_deref().unwrap_or("Not set")),
                Line::from(""),
                Line::from("Select a library to view details"),
                Line::from(""),
                Line::from("* Critical system libraries are marked with an asterisk"),
            ];
            lines
        };

        Paragraph::new(details)
            .block(Block::bordered().title("Library Details"))
            .render(layout[1], buf);
    }
}
