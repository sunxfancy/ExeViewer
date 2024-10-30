
use elf::ElfBytes;
use elf::endian::AnyEndian;
use elf::note::Note;
use elf::note::NoteGnuBuildId;
use elf::section::SectionHeader;
use ratatui::widgets::canvas::Shape;
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, NasmFormatter};

pub struct Elf<'a> {
    file_data: &'a Vec<u8>,
    pub elf: ElfBytes::<'a, AnyEndian>
}

impl<'a> Elf<'a> {
    pub fn new(file_data: &'a Vec<u8>) -> Self {
        let elf = ElfBytes::<AnyEndian>::minimal_parse(file_data).expect("Open elf file");

        Elf {
            file_data,  // 将 file_data 移入结构体
            elf,        // 传递给 ElfBytes 的引用在生命周期上与结构体匹配
        }
    }

    pub fn decompile_symbol(&self, symbol_address: u64, symbol_size: usize) -> String {
        // 读取内存片段
        let shdr = self.elf.section_header_by_name(".text").expect("Section not found");
        let (section, header) = self.elf.section_data(&shdr.unwrap()).expect("Section data not found");
        
        if (symbol_address < shdr.unwrap().sh_addr)  {
            return String::from("Symbol out of range");
        }

        let code_offset = (symbol_address - shdr.unwrap().sh_addr) as usize;

        if (code_offset + symbol_size) > shdr.unwrap().sh_size as usize {
            return String::from("Symbol out of range");
        }

        let code = &section[code_offset..code_offset + symbol_size];
    
        let mut decoder = Decoder::with_ip(64, code, symbol_address, DecoderOptions::NONE);
        let mut formatter = iced_x86::IntelFormatter::new();
    
        let mut instruction = Instruction::default();
        let mut buffer = String::new();
        while decoder.can_decode() {
            decoder.decode_out(&mut instruction);
            
            let mut output = String::new();
            formatter.format(&instruction, &mut output);
            
            buffer.push_str(output.as_str());
            buffer.push('\n');
        }
        buffer
    }
    

}
