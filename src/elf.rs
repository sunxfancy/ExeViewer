use std::collections::HashMap;
use std::mem;
use std::path::Path;

use elf::endian::AnyEndian;
use elf::segment::ProgramHeader;
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
        return vec![Line::from(format!(
            "Symbol out of range: {:08X}",
            symbol_address
        ))];
    }

    let code_offset = (symbol_address - shdr.unwrap().sh_addr) as usize;

    if (code_offset + symbol_size) > shdr.unwrap().sh_size as usize {
        return vec![Line::from(format!(
            "Symbol out of range: {:08X}",
            symbol_address
        ))];
    }

    let code: &[u8] = &section[code_offset..code_offset + symbol_size];
    let mut decoder = Decoder::with_ip(64, code, symbol_address, DecoderOptions::NONE);
    let resolver = MySymbolResolver::create_box(elf);
    let mut formatter: iced_x86::IntelFormatter =
        iced_x86::IntelFormatter::with_options(Some(resolver), None);

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

struct MySymbolResolver {
    addr_to_symbol: HashMap<u64, String>,
}

impl MySymbolResolver {
    pub fn new(elf: &ElfBytes<'_, AnyEndian>) -> MySymbolResolver {
        let mut addr_to_symbol = HashMap::new();
        // 解析符号表
        let sym_table = elf.symbol_table().expect("symtab should parse");
        match sym_table {
            Some((symbols, strtab)) => {
                for symbol in symbols.iter() {
                    if let Ok(name) = strtab.get(symbol.st_name as usize) {
                        addr_to_symbol.insert(symbol.st_value, name.to_string());
                    }
                }
            }
            None => {}
        };

        // 解析PLT
        if let Ok(Some(rela_plt)) = elf.section_header_by_name(".rela.plt") {
            if let Ok((rela_data, _)) = elf.section_data(&rela_plt) {
                if let Ok(Some(plt)) = elf.section_header_by_name(".plt") {
                    let rela = elf.section_data_as_relas(&rela_plt).unwrap();
                    let (dynsym, dynstr) = elf
                        .dynamic_symbol_table()
                        .expect("dynsym should parse")
                        .unwrap();
                    let _ = rela.enumerate().for_each(|(i, s)| {
                        let sym = dynsym.get(s.r_sym as usize).unwrap();
                        let name = dynstr.get(sym.st_name as usize).unwrap();

                        addr_to_symbol.insert(
                            plt.sh_addr + (i as u64 + 1) * plt.sh_entsize,
                            format!("{}@plt", name),
                        );
                    });
                }
            }
        }

        MySymbolResolver { addr_to_symbol }
    }

    pub fn create_box(elf: &ElfBytes<'_, AnyEndian>) -> Box<dyn SymbolResolver> {
        Box::new(Self::new(elf))
    }
}

impl SymbolResolver for MySymbolResolver {
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

        self.addr_to_symbol
            .get(&address)
            .map(|name| SymbolResult::with_str(address, name.as_str()))
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

pub fn get_interpreter(elf: &ElfBytes<AnyEndian>) -> Option<String> {
    // 遍历程序头表查找 PT_INTERP 段
    for ph in elf.segments().unwrap() {
        if ph.p_type == elf::abi::PT_INTERP {
            // 读取 INTERP 段的数据
            if let Ok(data) = elf.segment_data(&ph) {
                // 去掉结尾的 null 字节并转换为字符串
                return String::from_utf8(data[..data.len() - 1].to_vec()).ok();
            }
        }
    }
    None
}
