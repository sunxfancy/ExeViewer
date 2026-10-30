//! Portable, artifact-specific evidence. Files are supplied separately in a rootfs.
use super::{environment::Environment, Report};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct FileEvidence {
    pub path: String,
    pub resolved_path: Option<String>,
    pub sha256: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub kind: String,
    pub artifact_sha256: String,
    pub target_path: String,
    pub environment: Environment,
    pub files: Vec<FileEvidence>,
    pub cpu_flags: Vec<String>,
    pub default_paths_source: String,
    pub limitations: Vec<String>,
}
fn evidence(env: &Environment, path: &str) -> Result<FileEvidence, String> {
    match env.locate(path) {
        Ok(p) => Ok(FileEvidence {
            path: path.into(),
            resolved_path: Some(env.virtual_path(&p)),
            sha256: Some(format!(
                "{:x}",
                Sha256::digest(fs::read(&p).map_err(|e| format!("{path}: {e}"))?)
            )),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileEvidence {
            path: path.into(),
            resolved_path: None,
            sha256: None,
        }),
        Err(e) => Err(format!("{path}: {e}")),
    }
}
pub fn capture(r: &Report, explicit_defaults: bool) -> Result<Snapshot, String> {
    let main = r.objects.first().ok_or("empty diagnosis")?;
    let mut paths: BTreeSet<String> = r.objects.iter().skip(1).map(|o| o.path.clone()).collect();
    paths.extend(main.elf.interpreter.iter().cloned());
    for edge in &r.dependencies {
        paths.extend(edge.attempts.iter().map(|a| a.path.clone()));
    }
    paths.extend(["/etc/ld.so.cache", "/etc/ld.so.preload", "/etc/ld.so.conf"].map(String::from));
    // ld.so.conf is provenance; the runtime consumes the cache, not this file.
    if let Ok(dir) = r.environment.locate("/etc/ld.so.conf.d") {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_file() {
                paths.insert(format!(
                    "/etc/ld.so.conf.d/{}",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
    }
    let files = paths
        .iter()
        .map(|p| evidence(&r.environment, p))
        .collect::<Result<_, _>>()?;
    let cpu_flags = if r.environment.rootfs == Path::new("/") {
        let cpu = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let mut sets = cpu
            .lines()
            .filter_map(|line| line.split_once(':'))
            .filter(|(key, _)| key.trim() == "flags")
            .map(|(_, value)| {
                value
                    .split_whitespace()
                    .map(String::from)
                    .collect::<BTreeSet<_>>()
            });
        let first = sets.next().unwrap_or_default();
        sets.fold(first, |a, b| a.intersection(&b).cloned().collect())
            .into_iter()
            .collect()
    } else {
        vec![]
    };
    Ok(Snapshot { schema_version: 1, kind: "environment_snapshot".into(),
        artifact_sha256: main.elf.sha256.clone(), target_path: main.path.clone(),
        environment: r.environment.clone(), files, cpu_flags,
        default_paths_source: if explicit_defaults { "explicit input" } else { "assumed conventional x86-64 glibc directories" }.into(),
        limitations: vec!["Artifact-specific file evidence, not a complete rootfs archive or source authentication. Supply the target rootfs separately.".into(),
            "CPU flags are observations, not a proof of usable ISA; --isa-level remains an explicit target input.".into(),
            "Snapshot validation covers recorded files and absent search candidates; unsupported loader rules remain incomplete. Keep the filesystem unchanged during capture and diagnosis.".into()],
    })
}
impl Snapshot {
    pub fn verify(&self, artifact: &Path, rootfs: &Path) -> Result<(), String> {
        if super::elf_info::read(artifact)?.sha256 != self.artifact_sha256 {
            return Err("snapshot artifact SHA-256 mismatch".into());
        }
        if !rootfs.is_dir() {
            return Err("rootfs must be a directory".into());
        }
        let mut env = self.environment.clone();
        env.rootfs = rootfs.into();
        for file in &self.files {
            if evidence(&env, &file.path)? != *file {
                return Err(format!(
                    "snapshot evidence changed: {}; recapture the environment",
                    file.path
                ));
            }
        }
        Ok(())
    }
}
pub fn read(path: &Path) -> Result<Snapshot, String> {
    let s: Snapshot = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if s.schema_version != 1
        || s.kind != "environment_snapshot"
        || s.files.is_empty()
        || !s.target_path.starts_with('/')
    {
        return Err("expected nonempty schema_version 1 environment_snapshot".into());
    }
    Ok(s)
}
pub fn write_new<T: Serialize>(path: &PathBuf, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())
}
