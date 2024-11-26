use elf::{endian::AnyEndian, parse::ParsingTable, string_table::StringTable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, List, ListState, Paragraph, StatefulWidget, Widget},
};

pub struct SummaryPage<'a> {
    pub content: Vec<Section>,
    pub list: List<'a>,
    pub state: ListState,
}

pub struct Section {
    offset: u64,
    size: u64,
    description: String,
    data: String,
}

impl SummaryPage<'_> {
    pub fn new<'a>(
        sec_tab: ParsingTable<'a, AnyEndian, elf::section::SectionHeader>,
        str_tab: StringTable<'a>,
    ) -> SummaryPage<'a> {
        let name_list: Vec<&str> = sec_tab
            .iter()
            .map(|s| str_tab.get(s.sh_name as usize).unwrap())
            .collect();
        let list = List::new(name_list)
            .block(Block::bordered().title("Sections"))
            .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true);

        let content = sec_tab
            .iter()
            .map(|s| Section {
                offset: s.sh_offset,
                size: s.sh_size,
                description: getDescription(str_tab.get(s.sh_name as usize).unwrap()),
                data: String::new(),
            })
            .collect();

        SummaryPage {
            content,
            list,
            state: ListState::default(),
        }
    }
}

impl Widget for &mut SummaryPage<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(30), Constraint::Percentage(100)])
            .split(area);

        StatefulWidget::render(&self.list, layout[0], buf, &mut self.state);
        let selected = self.state.selected();

        let paragraph = Paragraph::new(if selected.is_none() {
            String::from("Select a section to show its details")
        } else {
            if selected.unwrap() >= self.content.len() {
                String::from("Section not found")
            } else {
                let section = &self.content[selected.unwrap()];
                format!(
                    "\n\n\
                    \x20       Description:  {}\n\n\
                    \x20       Size:  {}\n\n\
                    \x20       Range:  [ {:016X} - {:016X} ]\n\n",
                    section.description,
                    section.size,
                    section.offset,
                    section.offset + section.size
                )
            }
        })
        .block(Block::bordered().title("Section Summary"));

        paragraph.render(layout[1], buf);
    }
}

fn getDescription(name: &str) -> String {
    match name {
        ".text" => "Executable code".to_string(),
        ".rodata" => "Read-only data".to_string(),
        ".data" => "Initialized data".to_string(),
        ".bss" => "Uninitialized data".to_string(),
        ".symtab" => "Symbol table".to_string(),
        ".strtab" => "String table".to_string(),
        ".shstrtab" => "Section header string table".to_string(),
        _ => "Unknown".to_string(),
    }
}
