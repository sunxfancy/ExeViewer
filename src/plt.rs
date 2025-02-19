use elf::{
    endian::AnyEndian, parse::{ParsingIterator, ParsingTable}, relocation::Rela, section::SectionHeader, string_table::StringTable, symbol::SymbolTable, ElfBytes
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, List, ListDirection, ListState, Paragraph, StatefulWidget, Widget},
};

use crate::elf::decompile_symbol;

pub struct PLTPage<'a> {
    pub content: Vec<PLTItem<'a>>,
    pub list: List<'a>,
    pub state: ListState,
    active_on_content: bool,
}

pub struct PLTItem<'a> {
    address: u64, // 该项真实在内存中的地址
    size: u64, // 大小
    decompiled: bool, // 是否已反编译
    data: Vec<Line<'a>>, // 反编译数据
}

impl<'a> PLTPage<'a> {
    pub fn new(
        rela: ParsingIterator<'a, AnyEndian, Rela>,
        sym_tab: SymbolTable<'a, AnyEndian>,
        str_tab: StringTable<'a>,
        plt: SectionHeader,
    ) -> PLTPage<'a> {
        let name_list: Vec<&str> = rela
            .map(|s| {
                let sym = sym_tab.get(s.r_sym as usize).unwrap();
                str_tab.get(sym.st_name as usize).unwrap()
            })
            .collect();
        
        let mut content: Vec<PLTItem<'_>> = vec![];
        for i in 0..name_list.len() {
            content.push(PLTItem {
                address: plt.sh_addr + (i as u64 + 1) * plt.sh_entsize,
                size: plt.sh_entsize,
                decompiled: false,
                data: vec![],
            });
        }

        let list = List::new(name_list)
            .block(Block::bordered().title("Dynamic Symbols"))
            .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
            .highlight_symbol(">> ")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::TopToBottom);
        
        PLTPage {
            content,
            list,
            state: ListState::default(),
            active_on_content: false,
        }
    }

    pub fn load_symbol(&mut self, elf: &ElfBytes<'a, AnyEndian>, idx: usize) {
        if idx >= self.content.len() {
            return;
        }
        let symbol = &self.content[idx];
        if !symbol.decompiled {
            let decompiled: Vec<Line<'a>> =
                decompile_symbol(elf, symbol.address, symbol.size as usize, ".plt");
            self.content[idx].data = decompiled;
            self.content[idx].decompiled = true;
        }
    }

    pub fn select_next(&mut self, elf_file: &ElfBytes<'a, AnyEndian>) {
        self.state.select_next();
        let idx: usize = self.state.selected().unwrap();
        self.load_symbol(elf_file, idx);
    }

    pub fn select_previous(&mut self, elf_file: &ElfBytes<'a, AnyEndian>) {
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

impl Widget for &mut PLTPage<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(40), Constraint::Percentage(100)])
            .split(area);

        StatefulWidget::render(&self.list, layout[0], buf, &mut self.state);

        let selected = self.state.selected();
        if selected.is_none() {
            Paragraph::new("Select a symbol to decompile")
        } else {
            Paragraph::new(self.content[selected.unwrap()].data.clone())
        }
        .block(Block::bordered().title("PLT Table"))
        .render(layout[1], buf);
    }
}
