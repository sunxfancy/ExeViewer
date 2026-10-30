use super::{environment::Environment, Report};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Format {
    Text,
    Json,
}
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Capture portable environment evidence for one executable
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
    /// Diagnose static startup compatibility against a target filesystem
    Diagnose(DiagnoseArgs),
    /// Print ELF metadata and declared symbol/version requirements without execution
    Inspect {
        file: PathBuf,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Compare two saved diagnosis JSON reports (same input artifact)
    Compare {
        #[arg(long)]
        env_a: PathBuf,
        #[arg(long)]
        env_b: PathBuf,
        /// Link dependency differences to new failures using saved evidence
        #[arg(long)]
        explain: bool,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Import loader evidence, or explicitly execute a program to capture it
    Trace(super::trace::TraceArgs),
}
#[derive(Debug, Subcommand)]
pub enum EnvCommands {
    Capture {
        #[command(flatten)]
        args: DiagnoseArgs,
        #[arg(long)]
        output: PathBuf,
    },
}
#[derive(Clone, Debug, Args)]
pub struct DiagnoseArgs {
    pub file: PathBuf,
    /// Import a captured environment; verify its file evidence against --rootfs
    #[arg(long)]
    pub env: Option<PathBuf>,
    /// Explore this same diagnosis in an interactive terminal interface
    #[arg(long, conflicts_with = "format")]
    pub interactive: bool,
    /// Target root filesystem; dependencies never fall back to the host filesystem
    #[arg(long, default_value = "/")]
    pub rootfs: PathBuf,
    /// Installed path of the input ELF inside the target (used for $ORIGIN)
    #[arg(long)]
    pub target_path: Option<String>,
    /// Target working directory (defaults to / for an offline root)
    #[arg(long)]
    pub cwd: Option<String>,
    /// Colon-separated target LD_LIBRARY_PATH (explicit empty value clears it)
    #[arg(long)]
    pub library_path: Option<String>,
    /// Colon/space-separated target LD_PRELOAD
    #[arg(long)]
    pub preload: Option<String>,
    /// Colon-separated directories compiled into the target loader
    #[arg(long)]
    pub default_path: Option<String>,
    #[arg(long)]
    pub machine: Option<u16>,
    /// Target x86-64 ISA level (1..4); omitted means unknown
    #[arg(long, value_parser=clap::value_parser!(u8).range(1..=4))]
    pub isa_level: Option<u8>,
    /// Target kernel release for checking declared GNU ABI minimum
    #[arg(long)]
    pub kernel_release: Option<String>,
    #[arg(long, value_enum, default_value = "text")]
    pub format: Format,
}
fn output(text: &str, code: i32) -> Result<i32, String> {
    match io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => Ok(code),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(code),
        Err(e) => Err(e.to_string()),
    }
}
fn json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|s| s + "\n")
        .map_err(|e| e.to_string())
}
pub fn run(command: Commands) -> i32 {
    let format = match &command {
        Commands::Diagnose(a) => a.format,
        Commands::Env {
            command: EnvCommands::Capture { args, .. },
        } => args.format,
        Commands::Inspect { format, .. } | Commands::Compare { format, .. } => *format,
        Commands::Trace(a) => a.format,
    };
    match execute(command) {
        Ok(code) => code,
        Err(error) => {
            if matches!(format, Format::Json) {
                let _=output(&json(&serde_json::json!({"schema_version":1,"kind":"error","status":"incomplete","error":error})).unwrap(),2);
            } else {
                eprintln!("ExeViewer: {error}");
            }
            2
        }
    }
}
fn execute(command: Commands) -> Result<i32, String> {
    match command {
        Commands::Env {
            command: EnvCommands::Capture { args, output: path },
        } => {
            if args.env.is_some() || args.interactive {
                return Err("capture does not accept --env or --interactive".into());
            }
            let report = report_for(&args)?;
            let snapshot = super::snapshot::capture(&report, args.default_path.is_some())?;
            super::snapshot::write_new(&path, &snapshot)?;
            output(
                &json(
                    &serde_json::json!({"schema_version":1,"kind":"environment_capture","output":path,"diagnosis_status":report.status,"file_count":snapshot.files.len(),"limitations":snapshot.limitations}),
                )?,
                report.exit_code(),
            )
        }
        Commands::Diagnose(a) => {
            let r = report_for(&a)?;
            let code = r.exit_code();
            if a.interactive {
                super::view::show(&r)?;
                return Ok(code);
            }
            output(
                &if matches!(a.format, Format::Json) {
                    json(&r)?
                } else {
                    r.text()
                },
                code,
            )
        }
        Commands::Inspect { file, format } => {
            let elf = super::elf_info::read(&file)?;
            output(
                &if matches!(format, Format::Json) {
                    json(
                        &serde_json::json!({"schema_version":1,"kind":"inspection","file":file,"compatibility":"not_assessed","elf":elf}),
                    )?
                } else {
                    format!("ExeViewer static ELF inspection\nFile: {:?}\nSHA-256: {}\nMachine: {}\nELF type: {}\nInterpreter: {:?}\nSONAME: {:?}\nNEEDED: {:?}\nRPATH: {:?}\nRUNPATH: {:?}\nRequired ISA level: {:?}\nMinimum kernel: {:?}\nVersion requirements:\n{}\nVersion definitions: {:?}\nDynamic symbols: {} (use --format json for individual symbols)\nNotes: {:?}\nCompatibility: NOT ASSESSED\n", file, elf.sha256, elf.machine, elf.elf_type, elf.interpreter, elf.soname, elf.needed, elf.rpath, elf.runpath, elf.required_isa_level, elf.minimum_kernel, json(&elf.versions_needed)?, elf.versions_defined, elf.symbols.len(), elf.notes)
                },
                0,
            )
        }
        Commands::Compare {
            env_a,
            env_b,
            explain,
            format,
        } => {
            let read = |p: PathBuf| -> Result<Report, String> {
                serde_json::from_slice(&fs::read(p).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())
            };
            let a = read(env_a)?;
            let b = read(env_b)?;
            if a.schema_version != 1
                || b.schema_version != 1
                || a.kind != "diagnosis"
                || b.kind != "diagnosis"
            {
                return Err("expected schema_version 1 diagnosis reports".into());
            }
            let first_a = a.objects.first().ok_or("A report has no artifact")?;
            let first_b = b.objects.first().ok_or("B report has no artifact")?;
            if first_a.elf.sha256 != first_b.elf.sha256 {
                return Err("reports refer to different input artifacts (SHA-256 mismatch)".into());
            }
            // Match requesters by logical library identity, not deployment path.
            fn identity(r: &Report, path: &str) -> String {
                if r.objects.first().is_some_and(|o| o.path == path) {
                    return "<main>".into();
                }
                r.objects
                    .iter()
                    .find(|o| o.path == path)
                    .and_then(|o| o.elf.soname.clone())
                    .unwrap_or_else(|| path.into())
            }
            fn edges(
                r: &Report,
            ) -> std::collections::BTreeMap<(String, String), Vec<serde_json::Value>> {
                let mut result: std::collections::BTreeMap<_, Vec<_>> =
                    std::collections::BTreeMap::new();
                for e in &r.dependencies {
                    let hash = e
                        .selected
                        .as_ref()
                        .and_then(|s| r.objects.iter().find(|o| &o.path == s))
                        .map(|o| o.elf.sha256.clone());
                    result.entry((identity(r, &e.requester), e.needed.clone())).or_default().push(
                        serde_json::json!({"selected":e.selected,"sha256":hash,"attempts":e.attempts,"reused":e.reused}));
                }
                result
            }
            let ea = edges(&a);
            let eb = edges(&b);
            let keys: std::collections::BTreeSet<_> = ea.keys().chain(eb.keys()).cloned().collect();
            let mut changes = vec![];
            for (requester, needed) in keys {
                let key = (requester.clone(), needed.clone());
                if ea.get(&key) != eb.get(&key) {
                    changes.push(serde_json::json!({"requester":requester,"needed":needed,"a":ea.get(&key),"b":eb.get(&key)}));
                }
            }
            let diagnostic_changes = serde_json::to_value(&a.diagnostics).unwrap()
                != serde_json::to_value(&b.diagnostics).unwrap();
            let code = if a.exit_code() == 2 || b.exit_code() == 2 {
                2
            } else if !changes.is_empty() || diagnostic_changes {
                1
            } else {
                0
            };
            let value = serde_json::json!({"schema_version":1,"kind":"comparison","explanation":if explain {super::explain::explain(&a,&b)} else {serde_json::Value::Null},"artifact_sha256":first_a.elf.sha256,"status_a":a.status,"status_b":b.status,"changes":changes,"diagnostics_a":a.diagnostics,"diagnostics_b":b.diagnostics,"environment_a":a.environment,"environment_b":b.environment,"limitations":["Compares saved observations, not current filesystem contents. Changes include search evidence, selected paths and library content hashes."]});
            output(
                &if matches!(format, Format::Json) {
                    json(&value)?
                } else {
                    format!("ExeViewer environment comparison\nA: {}\nB: {}\nDependency differences: {}\n{}",a.status,b.status,changes.len(),json(&value)?)
                },
                code,
            )
        }
        Commands::Trace(args) => {
            let (value, code) = super::trace::run(&args)?;
            output(
                &if matches!(args.format, Format::Json) {
                    json(&value)?
                } else {
                    format!("ExeViewer loader evidence\n{}", json(&value)?)
                },
                code,
            )
        }
    }
}

pub(super) fn report_for(args: &DiagnoseArgs) -> Result<Report, String> {
    let a = args.clone();
    if let Some(path) = &a.env {
        let mut snapshot = super::snapshot::read(path)?;
        let rootfs = fs::canonicalize(&a.rootfs).map_err(|e| e.to_string())?;
        snapshot.verify(&a.file, &rootfs)?;
        snapshot.environment.rootfs = rootfs;
        let mut env = snapshot.environment;
        if let Some(v) = a.cwd {
            env.cwd = v;
        }
        if let Some(v) = a.library_path {
            env.library_path = if v.is_empty() {
                vec![]
            } else {
                v.split([':', ';']).map(String::from).collect()
            };
        }
        if let Some(v) = a.preload {
            env.preload = v
                .split([':', ' '])
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
        if let Some(v) = a.default_path {
            env.default_paths = v
                .split(':')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
        if let Some(v) = a.machine {
            env.target_machine = v;
        }
        if a.isa_level.is_some() {
            env.isa_level = a.isa_level;
        }
        if a.kernel_release.is_some() {
            env.kernel_release = a.kernel_release;
        }
        if !env.cwd.starts_with('/') || env.default_paths.iter().any(|p| !p.starts_with('/')) {
            return Err("environment paths must be absolute".into());
        }
        env.source =
            "verified captured file evidence; explicit command options override captured inputs"
                .into();
        let target = a.target_path.as_deref().unwrap_or(&snapshot.target_path);
        if !target.starts_with('/') {
            return Err("--target-path must be absolute".into());
        }
        return super::diagnose(&a.file, Some(target), env);
    }
    let rootfs = fs::canonicalize(&a.rootfs).map_err(|e| format!("rootfs: {e}"))?;
    if !rootfs.is_dir() {
        return Err("rootfs must be a directory".into());
    }
    let local = rootfs == std::path::Path::new("/");
    let cwd = a.cwd.unwrap_or_else(|| {
        if local {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .to_string_lossy()
                .into()
        } else {
            "/".into()
        }
    });
    if !cwd.starts_with('/') {
        return Err("--cwd must be an absolute target path".into());
    }
    if a.target_path.as_ref().is_some_and(|p| !p.starts_with('/')) {
        return Err("--target-path must be absolute inside the target".into());
    }
    let library = a.library_path.or_else(|| {
        if local {
            std::env::var("LD_LIBRARY_PATH").ok()
        } else {
            None
        }
    });
    let preload = a
        .preload
        .or_else(|| {
            if local {
                std::env::var("LD_PRELOAD").ok()
            } else {
                None
            }
        })
        .unwrap_or_default();
    let defaults: Vec<String> = a
        .default_path
        .unwrap_or_else(|| {
            "/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu:/lib64:/usr/lib64:/lib:/usr/lib".into()
        })
        .split(':')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if defaults.iter().any(|p| !p.starts_with('/')) {
        return Err("--default-path entries must be absolute target paths".into());
    }
    let env = Environment {
        rootfs,
        cwd,
        library_path: library
            .filter(|s| !s.is_empty())
            .map(|s| s.split([':', ';']).map(String::from).collect())
            .unwrap_or_default(),
        preload: preload
            .split([':', ' '])
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        default_paths: defaults,
        target_machine: a.machine.unwrap_or(62),
        isa_level: a.isa_level,
        kernel_release: a.kernel_release.or_else(|| {
            if local {
                fs::read_to_string("/proc/sys/kernel/osrelease")
                    .ok()
                    .map(|s| s.trim().into())
            } else {
                None
            }
        }),
        source: if local {
            "local filesystem; LD_LIBRARY_PATH/LD_PRELOAD inherited unless overridden"
        } else {
            "offline rootfs; no host environment search paths inherited"
        }
        .into(),
    };
    super::diagnose(&a.file, a.target_path.as_deref(), env)
}
