use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, Paragraph},
};
use elf::ElfBytes;
use elf::endian::AnyEndian;
use ratatui::prelude::*;

pub trait Page<'a> {
    fn select_next(&mut self, elf: &ElfBytes<'a, AnyEndian>);
    fn select_previous(&mut self, elf: &ElfBytes<'a, AnyEndian>);
    fn select_left(&mut self);
    fn select_right(&mut self);
    fn page_render(&mut self, area: Rect, buf: &mut Buffer);
}

pub struct EmptyPage {
    message: String,
}

impl EmptyPage {
    pub fn new() -> EmptyPage {
        EmptyPage {
            message: String::from("This ELF file does not contain a symbol table"),
        }
    }
}

impl<'a> Widget for &EmptyPage {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let paragraph = Paragraph::new(self.message.clone())
            .block(Block::bordered().title("Error"));
        paragraph.render(area, buf);
    }
}

impl<'a> Page<'a> for EmptyPage {
    fn select_next(&mut self, _elf: &ElfBytes<'a, AnyEndian>) {}
    fn select_previous(&mut self, _elf: &ElfBytes<'a, AnyEndian>) {}
    fn select_left(&mut self) {}
    fn select_right(&mut self) {}
    fn page_render(&mut self, area: Rect, buf: &mut Buffer) {
        self.render(area, buf);
    }
}
