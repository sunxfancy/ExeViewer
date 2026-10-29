//! Static inspection only: never invoke the inspected ELF or its interpreter.
use std::{fmt::Write as _, fs, io, path::{Path, PathBuf}};
use elf::{abi, endian::AnyEndian, ElfBytes};
use sha2::{Digest, Sha256};

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn resolve(path: &Path) -> io::Result<PathBuf> {
    if path.exists() {
        return Ok(path.to_owned());
    }
    if !path.is_absolute() && path.components().count() == 1 {
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join(path);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "ELF file not found"))
}

pub fn render(path: &Path) -> io::Result<String> {
    let path = resolve(path)?;
    let bytes = fs::read(&path)?;
    let elf = ElfBytes::<AnyEndian>::minimal_parse(&bytes).map_err(invalid)?;
    let mut out = String::from("ExeViewer static ELF report\n");
    writeln!(out, "File: {:?}", path).unwrap();
    writeln!(out, "Size: {} bytes", bytes.len()).unwrap();
    writeln!(out, "SHA-256: {:x}", Sha256::digest(&bytes)).unwrap();
    writeln!(out, "Class: {:?}", elf.ehdr.class).unwrap();
    writeln!(out, "Machine: {}", elf.ehdr.e_machine).unwrap();
    writeln!(out, "ELF type: {}", elf.ehdr.e_type).unwrap();
    writeln!(out, "Entry point: {:#x}", elf.ehdr.e_entry).unwrap();
    let mut interpreter = None;
    if let Some(segments) = elf.segments() {
        for segment in segments {
            if segment.p_type == abi::PT_INTERP {
                let data = elf.segment_data(&segment).map_err(invalid)?;
                interpreter = Some(String::from_utf8_lossy(data.strip_suffix(&[0]).unwrap_or(data)).into_owned());
            }
        }
    }
    writeln!(out, "Interpreter: {}", interpreter.map(|s| format!("{s:?}")).unwrap_or_else(|| "not declared".into())).unwrap();
    let dynamic = elf.dynamic().map_err(invalid)?;
    writeln!(out, "\nDeclared dynamic requirements:").unwrap();
    if let Some(dynamic) = dynamic {
        // A sectionless ELF can still run. Do not treat absent section metadata as
        // proof that there are no dependencies.
        let str_header = elf.section_header_by_name(".dynstr").map_err(invalid)?;
        if let Some(header) = str_header {
            let (data, _) = elf.section_data(&header).map_err(invalid)?;
            let strings = elf::string_table::StringTable::new(data);
            let mut count = 0;
            for entry in dynamic.iter() {
                let label = match entry.d_tag {
                    abi::DT_NEEDED => "NEEDED",
                    abi::DT_RPATH => "RPATH",
                    abi::DT_RUNPATH => "RUNPATH",
                    abi::DT_SONAME => "SONAME",
                    _ => continue,
                };
                let offset = usize::try_from(entry.d_val()).map_err(invalid)?;
                let value = strings.get(offset).map_err(invalid)?;
                writeln!(out, "  {label}: {value:?}").unwrap();
                count += 1;
            }
            if count == 0 {
                writeln!(out, "  No NEEDED/RPATH/RUNPATH/SONAME entries declared.").unwrap();
            }
        } else {
            writeln!(out, "  Unavailable: .dynstr section metadata is absent; dependencies are unknown.").unwrap();
        }
    } else {
        writeln!(out, "  No dynamic table.").unwrap();
    }
    writeln!(out, "\nCompatibility: NOT ASSESSED").unwrap();
    writeln!(out, "Static declarations only. No target execution or loader invocation.").unwrap();
    writeln!(out, "Dependency resolution, recursive dependencies, symbol versions and target-environment checks are not yet implemented in report mode.").unwrap();
    Ok(out)
}
