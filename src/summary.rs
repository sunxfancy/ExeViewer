use std::{fs::Metadata, path::PathBuf, time::SystemTime};

use elf::{endian::AnyEndian, file::FileHeader};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget},
};

pub struct SummaryPage {
    file_name: String,
    file_size: u64,
    file_modified: SystemTime,
    file_hash: String,
    elf_header: FileHeader<AnyEndian>,
    compiler_info: Option<String>,
}

impl SummaryPage {
    pub fn new(
        path: PathBuf,
        metadata: Metadata,
        file_hash: String,
        elf_header: FileHeader<AnyEndian>,
        compiler_info: Option<String>,
    ) -> SummaryPage {
        SummaryPage {
            file_name: path.file_name().unwrap().to_string_lossy().into_owned(),
            file_size: metadata.len(),
            file_modified: metadata.modified().unwrap(),
            file_hash,
            elf_header,
            compiler_info,
        }
    }

    fn get_machine_type(&self) -> &'static str {
        match self.elf_header.e_machine {
            0x3E => "x86-64",
            0x28 => "ARM",
            0xB7 => "AArch64",
            0x02 => "SPARC",
            0x03 => "x86",
            0x08 => "MIPS",
            0x14 => "PowerPC",
            0x15 => "PowerPC64",
            0x32 => "IA-64",
            0x3E => "AMD64",
            _ => "Unknown",
        }
    }

    fn get_file_type(&self) -> &'static str {
        match self.elf_header.e_type {
            1 => "Relocatable",
            2 => "Executable",
            3 => "Shared object",
            4 => "Core dump",
            _ => "Unknown",
        }
    }

    fn format_size(size: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if size >= GB {
            format!("{:.2} GB", size as f64 / GB as f64)
        } else if size >= MB {
            format!("{:.2} MB", size as f64 / MB as f64)
        } else if size >= KB {
            format!("{:.2} KB", size as f64 / KB as f64)
        } else {
            format!("{} B", size)
        }
    }
}

impl Widget for &SummaryPage {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = vec![
            Line::from(vec![
                Span::raw("File Name: "),
                Span::styled(&self.file_name, Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("File Size: "),
                Span::styled(
                    SummaryPage::format_size(self.file_size),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::raw("Last Modified: "),
                Span::styled(
                    humantime::format_rfc3339(self.file_modified).to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("SHA-256: "),
                Span::styled(&self.file_hash, Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("Architecture: "),
                Span::styled(self.get_machine_type(), Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::raw("File Type: "),
                Span::styled(self.get_file_type(), Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::raw("Entry Point: "),
                Span::styled(
                    format!("0x{:x}", self.elf_header.e_entry),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
        ];

        // Add compiler info if available
        let lines = if let Some(compiler) = self.compiler_info.as_deref() {
            [
                lines,
                vec![
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("Compiler Info: "),
                        Span::styled(compiler, Style::default().add_modifier(Modifier::BOLD)),
                    ]),
                ],
            ]
            .concat()
        } else {
            lines
        };

        Paragraph::new(lines)
            .block(Block::bordered().title("File Summary"))
            .render(area, buf);
    }
}
