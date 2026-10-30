use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VersionNeed {
    pub library: String,
    pub version: String,
    pub weak: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub version: Option<String>,
    pub default_version: bool,
    pub weak: bool,
    pub defined: bool,
    #[serde(default)]
    pub symbol_type: u8,
    #[serde(default)]
    pub version_index: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElfInfo {
    pub sha256: String,
    pub machine: u16,
    pub elf_type: u16,
    pub interpreter: Option<String>,
    pub needed: Vec<String>,
    pub soname: Option<String>,
    pub rpath: Option<String>,
    pub runpath: Option<String>,
    pub nodeflib: bool,
    pub versions_needed: Vec<VersionNeed>,
    pub versions_defined: Vec<String>,
    pub symbols: Vec<Symbol>,
    pub dynamic: bool,
    pub required_isa_level: Option<u8>,
    pub minimum_kernel: Option<[u32; 3]>,
    pub notes: Vec<String>,
}
struct Reader {
    data: Vec<u8>,
    loads: Vec<(u64, u64, u64)>,
}
impl Reader {
    fn bytes(&self, o: usize, n: usize) -> Result<&[u8], String> {
        self.data
            .get(o..o.checked_add(n).ok_or("offset overflow")?)
            .ok_or_else(|| "truncated ELF data".into())
    }
    fn u16(&self, o: usize) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes(o, 2)?.try_into().unwrap()))
    }
    fn u32(&self, o: usize) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(o, 4)?.try_into().unwrap()))
    }
    fn u64(&self, o: usize) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.bytes(o, 8)?.try_into().unwrap()))
    }
    fn addr(&self, a: u64, n: usize) -> Result<usize, String> {
        for &(v, o, s) in &self.loads {
            if let Some(d) = a.checked_sub(v) {
                if d.checked_add(n as u64).is_some_and(|e| e <= s) {
                    let p = usize::try_from(o.checked_add(d).ok_or("address overflow")?)
                        .map_err(|_| "address overflow")?;
                    self.bytes(p, n)?;
                    return Ok(p);
                }
            }
        }
        Err(format!("unmapped ELF address {a:#x}"))
    }
    fn string(&self, base: usize, size: usize, index: u64) -> Result<String, String> {
        let i = usize::try_from(index).map_err(|_| "string offset overflow")?;
        if i >= size {
            return Err("string offset out of range".into());
        }
        let b = self.bytes(base + i, size - i)?;
        let end = b
            .iter()
            .position(|&c| c == 0)
            .ok_or("unterminated ELF string")?;
        String::from_utf8(b[..end].to_vec()).map_err(|_| "non-UTF8 ELF name is unsupported".into())
    }
}
pub fn read(path: &Path) -> Result<ElfInfo, String> {
    parse(fs::read(path).map_err(|e| e.to_string())?)
}
fn parse(data: Vec<u8>) -> Result<ElfInfo, String> {
    let mut r = Reader {
        data,
        loads: vec![],
    };
    if r.bytes(0, 4)? != b"\x7fELF" {
        return Err("not an ELF file".into());
    }
    if r.bytes(4, 3)? != [2, 1, 1] {
        return Err("unsupported ELF: requires ELF64 little endian version 1".into());
    }
    r.bytes(0, 64)?;
    if r.u32(20)? != 1 {
        return Err("unsupported ELF header version".into());
    }
    let mut info = ElfInfo {
        sha256: format!("{:x}", Sha256::digest(&r.data)),
        machine: r.u16(18)?,
        elf_type: r.u16(16)?,
        interpreter: None,
        needed: vec![],
        soname: None,
        rpath: None,
        runpath: None,
        nodeflib: false,
        versions_needed: vec![],
        versions_defined: vec![],
        symbols: vec![],
        dynamic: false,
        required_isa_level: None,
        minimum_kernel: None,
        notes: vec![],
    };
    let phoff = usize::try_from(r.u64(32)?).map_err(|_| "program header overflow")?;
    let phsize = r.u16(54)? as usize;
    let phnum = r.u16(56)? as usize;
    if phnum == 65535 {
        return Err("extended program-header count unsupported".into());
    }
    if phnum > 0 && phsize != 56 {
        return Err("invalid program header size".into());
    }
    let mut dynrange = None;
    for i in 0..phnum {
        let p = phoff
            .checked_add(i * phsize)
            .ok_or("program header overflow")?;
        r.bytes(p, 56)?;
        let t = r.u32(p)?;
        let off = r.u64(p + 8)?;
        let addr = r.u64(p + 16)?;
        let size = r.u64(p + 32)?;
        let o = usize::try_from(off).map_err(|_| "offset overflow")?;
        let s = usize::try_from(size).map_err(|_| "size overflow")?;
        if t == 1 {
            addr.checked_add(size)
                .ok_or("PT_LOAD virtual address overflow")?;
            off.checked_add(size)
                .ok_or("PT_LOAD file offset overflow")?;
            if size > r.u64(p + 40)? {
                return Err("PT_LOAD file size exceeds memory size".into());
            }
            r.bytes(o, s)?;
            r.loads.push((addr, off, size));
        }
        if t == 2 {
            r.bytes(o, s)?;
            dynrange = Some((o, s));
        }
        if t == 3 {
            info.interpreter = Some(r.string(o, s, 0)?);
        }
        if t == 4 {
            r.bytes(o, s)?;
            let mut cursor = o;
            let end = o + s;
            while cursor < end {
                if end - cursor < 12 {
                    return Err("truncated ELF note".into());
                }
                let namesz = r.u32(cursor)? as usize;
                let descsz = r.u32(cursor + 4)? as usize;
                let kind = r.u32(cursor + 8)?;
                let name = cursor + 12;
                let desc = name.checked_add((namesz + 3) & !3).ok_or("note overflow")?;
                let next = desc.checked_add((descsz + 3) & !3).ok_or("note overflow")?;
                if next > end {
                    return Err("note exceeds segment".into());
                }
                if r.bytes(name, namesz)? == b"GNU\0" {
                    if kind == 1 && descsz == 16 && r.u32(desc)? == 0 {
                        let v = [r.u32(desc + 4)?, r.u32(desc + 8)?, r.u32(desc + 12)?];
                        info.minimum_kernel =
                            Some(info.minimum_kernel.map(|old| old.max(v)).unwrap_or(v));
                    }
                    if kind == 5 {
                        let mut prop = desc;
                        while prop < desc + descsz {
                            if desc + descsz - prop < 8 {
                                return Err("truncated GNU property".into());
                            }
                            let ty = r.u32(prop)?;
                            let n = r.u32(prop + 4)? as usize;
                            let next_prop = prop
                                .checked_add(8 + ((n + 7) & !7))
                                .ok_or("property overflow")?;
                            if next_prop > desc + descsz {
                                return Err("GNU property exceeds note".into());
                            }
                            if ty == 0xc0008002 && n == 4 {
                                let mask = r.u32(prop + 8)?;
                                if mask & !15 != 0 {
                                    info.notes
                                        .push("Unknown required x86 ISA property bits".into());
                                }
                                for level in 1..=4 {
                                    if mask & (1 << (level - 1)) != 0 {
                                        info.required_isa_level =
                                            Some(info.required_isa_level.unwrap_or(1).max(level));
                                    }
                                }
                            }
                            prop = next_prop;
                        }
                    }
                }
                cursor = next;
            }
        }
    }
    let Some((d, size)) = dynrange else {
        return Ok(info);
    };
    info.dynamic = true;
    let mut tags = BTreeMap::new();
    let mut needed = vec![];
    let mut terminated = false;
    for i in (0..size).step_by(16) {
        if size - i < 16 {
            return Err("truncated dynamic entry".into());
        }
        let tag = r.u64(d + i)?;
        let value = r.u64(d + i + 8)?;
        if tag == 0 {
            terminated = true;
            break;
        }
        if tag == 1 {
            needed.push(value);
        }
        tags.insert(tag, value);
    }
    if !terminated {
        return Err("dynamic table lacks DT_NULL".into());
    }
    let strsize = usize::try_from(*tags.get(&10).ok_or("missing DT_STRSZ")?)
        .map_err(|_| "string size overflow")?;
    let strbase = r.addr(*tags.get(&5).ok_or("missing DT_STRTAB")?, strsize)?;
    let string = |v| r.string(strbase, strsize, v);
    info.needed = needed.into_iter().map(string).collect::<Result<_, _>>()?;
    info.soname = tags.get(&14).map(|&v| string(v)).transpose()?;
    info.rpath = tags.get(&15).map(|&v| string(v)).transpose()?;
    info.runpath = tags.get(&29).map(|&v| string(v)).transpose()?;
    info.nodeflib = tags.get(&0x6ffffffb).is_some_and(|v| v & 0x800 != 0);
    if tags.get(&30).is_some_and(|v| v & 2 != 0) || tags.contains_key(&16) {
        info.notes
            .push("DT_SYMBOLIC symbol scope is not modeled".into());
    }
    if tags.get(&0x6ffffffb).is_some_and(|v| v & 0x8 != 0) {
        info.notes.push("DF_1_GROUP scope is not modeled".into());
    }
    if tags.contains_key(&0x7fffffff) || tags.contains_key(&0x7ffffffd) {
        info.notes
            .push("ELF FILTER/AUXILIARY loading is not modeled".into());
    }
    let mut req_indices = BTreeMap::new();
    let mut def_indices = BTreeMap::new();
    if let Some(&addr) = tags.get(&0x6ffffffe) {
        let count = *tags.get(&0x6fffffff).ok_or("missing DT_VERNEEDNUM")?;
        if count > 65536 {
            return Err("too many version requirements".into());
        }
        let mut a = addr;
        for idx in 0..count {
            let p = r.addr(a, 16)?;
            if r.u16(p)? != 1 {
                return Err("unsupported verneed version".into());
            }
            let lib = string(r.u32(p + 4)? as u64)?;
            let n = r.u16(p + 2)?;
            let mut aux = a
                .checked_add(r.u32(p + 8)? as u64)
                .ok_or("verneed overflow")?;
            for j in 0..n {
                let q = r.addr(aux, 16)?;
                let name = string(r.u32(q + 8)? as u64)?;
                req_indices.insert(r.u16(q + 6)? & 0x7fff, name.clone());
                info.versions_needed.push(VersionNeed {
                    library: lib.clone(),
                    version: name,
                    weak: r.u16(q + 4)? & 2 != 0,
                });
                if j + 1 < n {
                    let next = r.u32(q + 12)?;
                    if next == 0 {
                        return Err("short vernaux chain".into());
                    }
                    aux = aux.checked_add(next as u64).ok_or("vernaux overflow")?;
                }
            }
            if idx + 1 < count {
                let next = r.u32(p + 12)?;
                if next == 0 {
                    return Err("short verneed chain".into());
                }
                a = a.checked_add(next as u64).ok_or("verneed overflow")?;
            }
        }
    }
    if let Some(&addr) = tags.get(&0x6ffffffc) {
        let count = *tags.get(&0x6ffffffd).ok_or("missing DT_VERDEFNUM")?;
        if count > 65536 {
            return Err("too many version definitions".into());
        }
        let mut a = addr;
        for idx in 0..count {
            let p = r.addr(a, 20)?;
            if r.u16(p)? != 1 {
                return Err("unsupported verdef version".into());
            }
            if r.u16(p + 6)? == 0 {
                return Err("empty version definition".into());
            }
            let q = r.addr(
                a.checked_add(r.u32(p + 12)? as u64)
                    .ok_or("verdef overflow")?,
                8,
            )?;
            let name = string(r.u32(q)? as u64)?;
            def_indices.insert(r.u16(p + 4)? & 0x7fff, name.clone());
            info.versions_defined.push(name);
            if idx + 1 < count {
                let next = r.u32(p + 16)?;
                if next == 0 {
                    return Err("short verdef chain".into());
                }
                a = a.checked_add(next as u64).ok_or("verdef overflow")?;
            }
        }
    }
    if let Some(&symaddr) = tags.get(&6) {
        if tags.get(&11) != Some(&24) {
            return Err("invalid DT_SYMENT".into());
        }
        let mut count = if let Some(&hash) = tags.get(&4) {
            let p = r.addr(hash, 8)?;
            r.u32(p + 4)? as usize
        } else if let Some(&hash) = tags.get(&0x6ffffef5) {
            let p = r.addr(hash, 16)?;
            let buckets = r.u32(p)? as usize;
            let offset = r.u32(p + 4)? as usize;
            let bloom = r.u32(p + 8)? as usize;
            if buckets > 1_000_000 || bloom > 1_000_000 {
                return Err("oversized GNU hash".into());
            }
            let baddr = hash
                .checked_add(16 + 8 * bloom as u64)
                .ok_or("hash overflow")?;
            let b = r.addr(baddr, buckets * 4)?;
            let mut max = 0;
            for i in 0..buckets {
                max = max.max(r.u32(b + i * 4)? as usize)
            }
            if max == 0 {
                offset
            } else {
                if max < offset {
                    return Err("bad GNU hash bucket".into());
                }
                let chain = baddr + buckets as u64 * 4;
                let mut i = max;
                loop {
                    if i > 1_000_000 {
                        return Err("oversized GNU hash chain".into());
                    }
                    let q = r.addr(chain + (i - offset) as u64 * 4, 4)?;
                    i += 1;
                    if r.u32(q)? & 1 != 0 {
                        break;
                    }
                }
                i
            }
        } else {
            info.notes
                .push("No dynamic hash: symbol inventory unavailable".into());
            0
        };
        // GNU hash can have no exported symbols: its symoffset then does not
        // bound undefined imports. Include all symbol indices used by dynamic
        // relocations, without relying on a section table.
        for (address_tag, size_tag, entry_size) in [
            (7, 8, 24usize),
            (17, 18, 16usize),
            (23, 2, if tags.get(&20) == Some(&17) { 16 } else { 24 }),
        ] {
            if let Some(&address) = tags.get(&address_tag) {
                let size = usize::try_from(*tags.get(&size_tag).ok_or("missing relocation size")?)
                    .map_err(|_| "relocation size overflow")?;
                if size % entry_size != 0 || size / entry_size > 1_000_000 {
                    return Err("invalid relocation table size".into());
                }
                let base = r.addr(address, size)?;
                for offset in (0..size).step_by(entry_size) {
                    count = count.max((r.u64(base + offset + 8)? >> 32) as usize + 1);
                }
            }
        }
        if count > 1_000_000 {
            return Err("too many symbols".into());
        }
        let base = r.addr(symaddr, count * 24)?;
        let vers = tags
            .get(&0x6ffffff0)
            .map(|&v| r.addr(v, count * 2))
            .transpose()?;
        for i in 1..count {
            let p = base + i * 24;
            let name = string(r.u32(p)? as u64)?;
            let bind = r.bytes(p + 4, 1)?[0] >> 4;
            if bind == 10 && !info.notes.iter().any(|n| n.contains("GNU_UNIQUE")) {
                info.notes
                    .push("GNU_UNIQUE symbol scope is not modeled".into());
            }
            let visibility = r.bytes(p + 5, 1)?[0] & 3;
            if name.is_empty() || bind == 0 {
                continue;
            }
            let defined = r.u16(p + 6)? != 0;
            if defined && (visibility == 1 || visibility == 2) {
                continue;
            }
            let v = if let Some(v) = vers {
                r.u16(v + i * 2)?
            } else {
                1
            };
            let version = if v & 0x7fff > 1 {
                let name = if defined {
                    def_indices
                        .get(&(v & 0x7fff))
                        .or_else(|| req_indices.get(&(v & 0x7fff)))
                } else {
                    req_indices.get(&(v & 0x7fff))
                };
                Some(name.ok_or("unresolved symbol version index")?.clone())
            } else {
                None
            };
            info.symbols.push(Symbol {
                name,
                version,
                default_version: v & 0x8000 == 0,
                weak: bind == 2,
                defined,
                symbol_type: r.bytes(p + 4, 1)?[0] & 15,
                version_index: v & 0x7fff,
            });
        }
    }
    info.versions_defined.sort();
    info.versions_defined.dedup();
    Ok(info)
}
