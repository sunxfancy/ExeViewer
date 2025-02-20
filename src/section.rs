use elf::{endian::AnyEndian, parse::ParsingTable, string_table::StringTable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, List, ListState, Paragraph, StatefulWidget, Widget},
};

pub struct SectionPage<'a> {
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

impl SectionPage<'_> {
    pub fn new<'a>(
        sec_tab: ParsingTable<'a, AnyEndian, elf::section::SectionHeader>,
        str_tab: StringTable<'a>,
    ) -> SectionPage<'a> {
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

        SectionPage {
            content,
            list,
            state: ListState::default(),
        }
    }
}

impl Widget for &mut SectionPage<'_> {
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
                let visualization = generate_section_visualization(&self.content, selected.unwrap(), 50, 3);
                format!(
                    "\n\n\
                    \x20       Description:  {}\n\n\
                    \x20       Size:  {}\n\n\
                    \x20       Range:  [ {:016X} - {:016X} ]\n\n\
                    \x20       Layout:\n{}\n",
                    section.description,
                    section.size,
                    section.offset,
                    section.offset + section.size,
                    visualization
                )
            }
        })
        .block(Block::bordered().title("Section Summary"));

        paragraph.render(layout[1], buf);
    }
}

fn generate_section_visualization(sections: &[Section], selected_idx: usize, width: usize, height: usize) -> String {
    let total_len = width * height;
    let mut visualization = vec!['.'; total_len];
    
    if let Some(max_offset) = sections.iter().map(|s| s.offset + s.size).max() {
        // 计算选中段在总长度中的起止位置
        let section = &sections[selected_idx];
        let start_pos = ((section.offset as f64 / max_offset as f64) * total_len as f64) as usize;
        let mut end_pos = (((section.offset + section.size) as f64 / max_offset as f64) * total_len as f64) as usize;
        
        // 确保小段至少显示一个字符
        if end_pos <= start_pos {
            end_pos = start_pos + 1;
        }
        end_pos = end_pos.min(total_len);
        
        // 标记区间
        for i in start_pos..end_pos {
            visualization[i] = '*';
        }
    }
    
    // 按照指定宽度分行输出
    (0..height)
        .map(|row| {
            let start = row * width;
            let end = start + width;
            format!("\x20       {}", visualization[start..end].iter().collect::<String>())
        })
        .collect::<Vec<_>>()
        .join("\n")
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
