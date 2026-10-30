use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Environment {
    pub rootfs: PathBuf,
    pub cwd: String,
    pub library_path: Vec<String>,
    pub preload: Vec<String>,
    pub default_paths: Vec<String>,
    pub target_machine: u16,
    pub isa_level: Option<u8>,
    pub kernel_release: Option<String>,
    pub source: String,
}
impl Environment {
    /// Resolve absolute symlinks against the target root, never the host root.
    pub fn locate(&self, path: &str) -> io::Result<PathBuf> {
        let absolute = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            Path::new(&self.cwd).join(path)
        };
        let mut pending: VecDeque<_> = absolute
            .components()
            .map(|c| c.as_os_str().to_os_string())
            .collect();
        let mut parts = PathBuf::new();
        let mut links = 0;
        while let Some(part) = pending.pop_front() {
            if part == "/" {
                parts.clear();
                continue;
            }
            if part == "." {
                continue;
            }
            if part == ".." {
                parts.pop();
                continue;
            }
            let next = self.rootfs.join(&parts).join(&part);
            let meta = fs::symlink_metadata(&next)?;
            if meta.file_type().is_symlink() {
                links += 1;
                if links > 40 {
                    return Err(io::Error::other("too many target symlinks"));
                }
                let dest = fs::read_link(&next)?;
                let components: Vec<_> = dest
                    .components()
                    .map(|c| c.as_os_str().to_os_string())
                    .collect();
                for c in components.into_iter().rev() {
                    pending.push_front(c)
                }
            } else {
                parts.push(part)
            }
        }
        Ok(self.rootfs.join(parts))
    }
    pub fn virtual_path(&self, path: &Path) -> String {
        format!(
            "/{}",
            path.strip_prefix(&self.rootfs)
                .unwrap_or(path)
                .to_string_lossy()
                .trim_start_matches('/')
        )
    }
    pub fn absolute(&self, path: &str) -> String {
        let p = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            Path::new(&self.cwd).join(path)
        };
        // Preserve '..' until after resolving symlinks in locate(). Lexically
        // collapsing it here can select a different library than the kernel.
        p.components()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned()
    }
}
#[derive(Clone)]
pub struct CacheEntry {
    pub name: String,
    pub path: String,
    pub hwcap: u64,
}
/// glibc's current cache format; unknown/legacy formats explicitly remain unknown.
pub fn cache(env: &Environment) -> Result<Vec<CacheEntry>, String> {
    let path = match env.locate("/etc/ld.so.cache") {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let b = fs::read(path).map_err(|e| e.to_string())?;
    if b.get(..20) != Some(b"glibc-ld.so.cache1.1") {
        return Err("unsupported ld.so.cache format".into());
    }
    let u32at = |p: usize| -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            b.get(p..p + 4)
                .ok_or("truncated loader cache")?
                .try_into()
                .unwrap(),
        ))
    };
    let n = u32at(20)? as usize;
    if n > 1_000_000 {
        return Err("oversized loader cache".into());
    }
    let string = |p: usize| -> Result<String, String> {
        let s = b.get(p..).ok_or("cache string out of bounds")?;
        let n = s
            .iter()
            .position(|v| *v == 0)
            .ok_or("unterminated cache string")?;
        String::from_utf8(s[..n].to_vec()).map_err(|_| "invalid cache string".into())
    };
    // Flags byte encodes endianness on newer glibc versions. Our supported target is LE.
    if b.get(28).is_some_and(|v| v & 3 == 3) {
        return Err("big-endian cache unsupported".into());
    }
    let mut out = vec![];
    for i in 0..n {
        let p = 48 + i * 24;
        let flags = u32at(p)?;
        let hwcap = u64::from_le_bytes(
            b.get(p + 16..p + 24)
                .ok_or("truncated loader cache")?
                .try_into()
                .unwrap(),
        );
        if flags & 0xff00 == 0x0300 {
            out.push(CacheEntry {
                name: string(u32at(p + 4)? as usize)?,
                path: string(u32at(p + 8)? as usize)?,
                hwcap,
            });
        }
    }
    Ok(out)
}
