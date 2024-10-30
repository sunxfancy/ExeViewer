
use elf::ElfBytes;
use elf::endian::AnyEndian;
use elf::note::Note;
use elf::note::NoteGnuBuildId;
use elf::section::SectionHeader;
use std::path::PathBuf;

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
}
