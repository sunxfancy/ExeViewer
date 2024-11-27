use std::mem;
use std::rc::Rc;
use std::sync::Arc;

use elf::endian::AnyEndian;
use elf::note::Note;
use elf::note::NoteGnuBuildId;
use elf::parse::ParsingTable;
use elf::section::SectionHeader;
use elf::string_table::StringTable;
use elf::symbol::Symbol;
use elf::ElfBytes;
use iced_x86::FormatterOutput;
use iced_x86::FormatterTextKind;
use iced_x86::SymbolResolver;
use iced_x86::SymbolResult;
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::canvas::Shape;

pub fn parse(file_data: &Vec<u8>) -> ElfBytes<'_, AnyEndian> {
    ElfBytes::<AnyEndian>::minimal_parse(file_data).expect("Open elf file")
}

pub fn decompile_symbol<'a>(
    elf: &ElfBytes<'a, AnyEndian>,
    symbol_address: u64,
    symbol_size: usize,
    section_name: &str,
) -> Vec<Line<'a>> {
    // 读取内存片段
    let shdr = elf
        .section_header_by_name(section_name)
        .expect("Section not found");
    let (section, _header) = elf
        .section_data(&shdr.unwrap())
        .expect("Section data not found");

    if symbol_address < shdr.unwrap().sh_addr {
        return vec![Line::from(format!("Symbol out of range: {:08X}", symbol_address))];
    }

    let code_offset = (symbol_address - shdr.unwrap().sh_addr) as usize;

    if (code_offset + symbol_size) > shdr.unwrap().sh_size as usize {
        return vec![Line::from(format!("Symbol out of range: {:08X}", symbol_address))];
    }

    let code: &[u8] = &section[code_offset..code_offset + symbol_size];

    // 解析符号表

    let resolver = MySymbolResolver::create_box(elf);
    let mut decoder = Decoder::with_ip(64, code, symbol_address, DecoderOptions::NONE);
    let mut formatter = iced_x86::IntelFormatter::with_options(Some(resolver), None);

    let mut instruction = Instruction::default();
    let mut buffer: Vec<Line<'a>> = vec![];
    while decoder.can_decode() {
        decoder.decode_out(&mut instruction);

        let mut output = MyFormatterOutput::new();
        formatter.format(&instruction, &mut output);

        let mut line_buf = vec![];
        line_buf.push(Span::from(format!("    {:016X}    ", instruction.ip())));

        for (text, kind) in output.vec {
            line_buf.push(get_color(text, kind));
        }

        buffer.push(Line::from(line_buf));
    }
    buffer
}

struct MySymbolResolver<'a> {
    symbols: ParsingTable<'a, AnyEndian, Symbol>,
    strtab: StringTable<'a>,
}

impl<'a> MySymbolResolver<'a> {
    pub fn create_box(elf: &ElfBytes<'a, AnyEndian>) -> Box<dyn SymbolResolver> {
        let sym_table = elf.symbol_table().expect("symtab should parse");
        let (symbols, strtab) = sym_table.unwrap();
        unsafe {
            let raw_box = Box::new(MySymbolResolver { symbols, strtab });

            // 将 Box<ElfSymbolResolver> 转换为 Box<dyn SymbolResolver>
            mem::transmute::<Box<dyn SymbolResolver + 'a>, Box<dyn SymbolResolver>>(raw_box)
        }
    }
}

impl SymbolResolver for MySymbolResolver<'_> {
    fn symbol(
        &mut self,
        _instruction: &Instruction,
        _operand: u32,
        _instruction_operand: Option<u32>,
        address: u64,
        _address_size: u32,
    ) -> Option<SymbolResult> {
        if !(_instruction.is_call_far() || _instruction.is_call_near()) {
            return None;
        }

        let symbol_string = self.strtab.get(address as usize);
        match symbol_string {
            // The 'address' arg is the address of the symbol and doesn't have to be identical
            // to the 'address' arg passed to symbol(). If it's different from the input
            // address, the formatter will add +N or -N, eg. '[rax+symbol+123]'
            Ok(str) => Some(SymbolResult::with_str(address, str)),
            Err(_) => None,
        }
    }
}

// Custom formatter output that stores the output in a vector.
struct MyFormatterOutput {
    vec: Vec<(String, FormatterTextKind)>,
}

impl MyFormatterOutput {
    pub fn new() -> Self {
        Self { vec: Vec::new() }
    }
}

impl FormatterOutput for MyFormatterOutput {
    fn write(&mut self, text: &str, kind: FormatterTextKind) {
        // This allocates a string. If that's a problem, just call print!() here
        // instead of storing the result in a vector.
        self.vec.push((String::from(text), kind));
    }
}

fn get_color<'a>(s: String, kind: FormatterTextKind) -> Span<'a> {
    match kind {
        FormatterTextKind::Directive | FormatterTextKind::Keyword => {
            Span::styled(s, Style::new().yellow().italic())
        }
        FormatterTextKind::Prefix | FormatterTextKind::Mnemonic => {
            Span::styled(s, Style::default().bold())
        }
        FormatterTextKind::Register => Span::styled(s, Style::new().green()),
        FormatterTextKind::Number => Span::styled(s, Style::new().cyan()),
        _ => Span::styled(s, Style::default()),
    }
}
