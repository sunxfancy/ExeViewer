use std::vec;

use elf::{endian::AnyEndian, parse::ParsingTable, string_table::StringTable};
use elf::ElfBytes;

use ratatui::text::Line;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, List, ListDirection, ListState, Paragraph, StatefulWidget, Widget},
};
use crate::elf::decompile_symbol;

pub struct SymbolPage<'a> {
    pub content: Vec<Symbol<'a>>,
    pub list: List<'a>,
    pub state: ListState,
    pub active_on_content: bool,
}

pub struct Symbol<'a> {
    address: u64,
    size: u64,
    decompiled: bool,
    data: Vec<Line<'a>>,
}

impl <'a> SymbolPage<'a> {
    pub fn new(sym_tab: ParsingTable<'a, AnyEndian, elf::symbol::Symbol>, str_tab: StringTable<'a>) -> SymbolPage<'a> {
        let mut name_list: Vec<&str> = Vec::new();
        let mut content: Vec<Symbol> = Vec::new();
        sym_tab.iter().for_each(|sym| {
            let name = str_tab.get(sym.st_name as usize).unwrap();
            name_list.push(name);
            content.push(Symbol {
                address: sym.st_value,
                size: sym.st_size,
                decompiled: false,
                data: vec![],
            });
        });

        let list = List::new(name_list)
            .block(Block::bordered().title("Symbols"))
            .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::TopToBottom);

        SymbolPage {
            content,
            list,
            state: ListState::default(),
            active_on_content: false
        }
    }

    pub fn load_symbol(&mut self, elf: &ElfBytes::<'a, AnyEndian>, idx: usize) {
        if idx >= self.content.len() {
            return;
        }
        let symbol = &self.content[idx];
        if !symbol.decompiled {
            let decompiled :Vec<Line<'a>> = decompile_symbol(elf, symbol.address, symbol.size as usize);
            self.content[idx].data = decompiled;
            self.content[idx].decompiled = true;
        }
    }

    pub fn select_next(&mut self, elf_file: &ElfBytes::<'a, AnyEndian>) {
        self.state.select_next();
        let idx: usize = self.state.selected().unwrap();
        self.load_symbol(elf_file, idx);
    }

    pub fn select_previous(&mut self, elf_file: &ElfBytes::<'a, AnyEndian>) {
        self.state.select_previous();
        let idx: usize = self.state.selected().unwrap();
        self.load_symbol(elf_file, idx);
    }

    pub fn select_left(&mut self) {
        self.active_on_content = false;
    }

    pub fn select_right(&mut self) {
        self.active_on_content = true;
    }
}

impl Widget for &mut SymbolPage<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(40),Constraint::Percentage(100)])
            .split(area);

        StatefulWidget::render(&self.list, layout[0], buf, &mut self.state);
        let selected = self.state.selected();

        if selected.is_none() {
            Paragraph::new("Select a symbol to decompile")
        } else {
            Paragraph::new(self.content[selected.unwrap()].data.clone())
        }
        .block(Block::bordered().title("Assembly"))
        .render(layout[1], buf);
    }
}
