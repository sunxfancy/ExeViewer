use super::{cli::Format, Report};
use clap::Args;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, Args)]
pub struct TraceArgs {
    /// Existing glibc LD_DEBUG log. Import does not execute anything.
    #[arg(long, conflicts_with_all=["execute","file","output_dir","args"])]
    pub log: Option<PathBuf>,
    /// Explicitly run FILE on this machine with LD_DEBUG enabled (not a sandbox)
    #[arg(long, requires_all=["file","output_dir"], conflicts_with="log")]
    pub execute: bool,
    /// Executable to run; requires --execute
    #[arg(requires = "execute")]
    pub file: Option<PathBuf>,
    /// New directory for target stdout/stderr and loader logs; must not exist
    #[arg(long, requires = "execute")]
    pub output_dir: Option<PathBuf>,
    /// Stop the captured process group after this many seconds
    #[arg(long,default_value_t=15,value_parser=clap::value_parser!(u64).range(1..=3600))]
    pub timeout: u64,
    /// Compare mapped/initialized libraries with a saved diagnosis report
    #[arg(long)]
    pub baseline: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "text")]
    pub format: Format,
    #[arg(last = true)]
    pub args: Vec<String>,
}
// LD_DEBUG records carry a decimal PID prefix. Application stderr does not
// become loader evidence merely because it contains one of the same words.
fn events(text: &str, source: &str) -> Vec<Value> {
    text.lines().enumerate().filter_map(|(i, line)| {
        let (prefix, body) = line.trim_start().split_once(':')?;
        let pid = prefix.parse::<u32>().ok();
        let body = body.trim();
        let kind = if pid.is_some() {
            if body.starts_with("find library=") { "search" }
            else if body.starts_with("search path=") { "search_path" }
            else if body.starts_with("trying file=") { "candidate" }
            else if body.starts_with("file=") && body.ends_with("generating link map") { "mapped" }
            else if body.starts_with("calling init:") { "initialize" }
            else if body.starts_with("calling fini:") { "finalize" }
            else if body.starts_with("file=") && body.contains("dynamically loaded by") { "dynamic_load" }
            else if body.starts_with("file=") && body.contains("needed by") { "dependency" }
            else if body.starts_with("binding file ") { "binding" }
            else if body.starts_with("checking for version ") { "version_check" }
            else if body.contains(": error: ") && body.ends_with("(fatal)") { "loader_error" }
            else { return None }
        } else if body.starts_with("error while loading shared libraries: ")
            || body.starts_with("symbol lookup error: ")
            || (body.contains(": version `") && body.contains("' not found (required by ") && body.ends_with(')')) {
            "loader_error"
        } else { return None };
        let object = if kind == "initialize" || kind == "finalize" {
            body.split_once(':').map(|(_, s)| s.trim().to_string())
        } else if kind == "mapped" {
            body.strip_prefix("file=").and_then(|s| s.split_once(" [")).map(|(s, _)| s.trim().to_string())
        } else { None };
        let mut event = json!({"source":source,"line":i+1,"pid":pid,"kind":kind,"object":object,"evidence":line});
        if kind == "binding" {
            // Preserve namespace IDs: the same pathname can exist in multiple scopes.
            let binding = body.strip_prefix("binding file ")?;
            let (requester, rest) = binding.split_once(" [")?;
            let (requester_namespace, rest) = rest.split_once("] to ")?;
            let (provider, rest) = rest.split_once(" [")?;
            let (provider_namespace, rest) = rest.split_once("]: ")?;
            let (_, symbol) = rest.split_once(" symbol `")?;
            let (symbol, rest) = symbol.split_once('\'')?;
            let version = rest.trim().strip_prefix('[').and_then(|s| s.strip_suffix(']'));
            event["binding"] = json!({"requester":requester,"provider":provider,
                "requester_namespace":requester_namespace,"provider_namespace":provider_namespace,
                "symbol":symbol,"version":version,"certainty":"observed"});
        }
        Some(event)
    }).collect()
}
#[cfg(unix)]
fn capture(a: &TraceArgs) -> Result<(Vec<Value>, Value, i32), String> {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let file = fs::canonicalize(
        a.file
            .as_ref()
            .ok_or("trace requires FILE with --execute")?,
    )
    .map_err(|e| e.to_string())?;
    let elf = super::elf_info::read(&file)?;
    if elf.machine != 62
        || !elf
            .interpreter
            .as_ref()
            .is_some_and(|s| s.ends_with("ld-linux-x86-64.so.2"))
    {
        return Err("capture requires an x86-64 glibc dynamically linked executable".into());
    }
    let out = a.output_dir.as_ref().ok_or("--output-dir required")?;
    fs::create_dir(out).map_err(|e| format!("create new trace directory: {e}"))?;
    let out = fs::canonicalize(out).map_err(|e| e.to_string())?;
    let stdout = fs::File::create(out.join("stdout.log")).map_err(|e| e.to_string())?;
    let stderr = fs::File::create(out.join("stderr.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(&file);
    command
        .args(&a.args)
        .env("LD_DEBUG", "libs,files,versions,bindings")
        .env("LD_DEBUG_OUTPUT", out.join("loader"))
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .process_group(0);
    struct CaptureChild(std::process::Child);
    impl Drop for CaptureChild {
        fn drop(&mut self) {
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.wait();
        }
    }
    // Also clean up on I/O errors while monitoring, not just on timeout.
    let mut child = CaptureChild(command.spawn().map_err(|e| e.to_string())?);
    let pid = child.0.id();
    let start = Instant::now();
    let mut stopped = None;
    let status = loop {
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        let total: u64 = fs::read_dir(&out)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        if start.elapsed() > Duration::from_secs(a.timeout) || total > 32 * 1024 * 1024 {
            stopped = Some(if total > 32 * 1024 * 1024 {
                "capture size limit"
            } else {
                "timeout"
            });
            // The child owns a separate process group; do not leave traced descendants running.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            break child.0.wait().map_err(|e| e.to_string())?;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    // Clean up background children still in the capture's group after its parent exits.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    drop(child);
    let mut logs: Vec<_> = fs::read_dir(&out)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("loader."))
        })
        .collect();
    logs.sort();
    logs.push(out.join("stderr.log"));
    let mut all = vec![];
    for path in logs {
        let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
        if meta.len() > 32 * 1024 * 1024 {
            return Err("trace log exceeds 32 MiB; raw evidence retained".into());
        }
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        all.extend(events(&text, &path.to_string_lossy()));
    }
    let code = if stopped.is_some() || all.is_empty() {
        2
    } else if !status.success() {
        1
    } else {
        0
    };
    let metadata = json!({"file":file,"artifact_sha256":elf.sha256,"arguments":a.args,"cwd":std::env::current_dir().ok(),"output_dir":out,"pid":pid,"target_exit_code":status.code(),"signal":status.signal(),"stopped":stopped,"library_path":std::env::var("LD_LIBRARY_PATH").ok(),"preload":std::env::var("LD_PRELOAD").ok(),"instrumentation":"LD_DEBUG=libs,files,versions,bindings; stdin closed; stdout/stderr captured; isolated process group"});
    Ok((all, metadata, code))
}
#[cfg(not(unix))]
fn capture(_a: &TraceArgs) -> Result<(Vec<Value>, Value, i32), String> {
    Err("capture is supported only on Unix hosts".into())
}
pub fn run(a: &TraceArgs) -> Result<(Value, i32), String> {
    let baseline = a
        .baseline
        .as_ref()
        .map(|path| -> Result<Report, String> {
            let r: Report = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if r.schema_version != 1 || r.kind != "diagnosis" || r.objects.is_empty() {
                return Err("baseline must be a nonempty schema_version 1 diagnosis".into());
            }
            if a.execute {
                let elf = super::elf_info::read(a.file.as_ref().ok_or("FILE required")?)?;
                if r.objects[0].elf.sha256 != elf.sha256 {
                    return Err("baseline artifact does not match executable".into());
                }
            }
            Ok(r)
        })
        .transpose()?;
    let (all, metadata, mut code) = if let Some(log) = &a.log {
        if fs::metadata(log).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
            return Err("log exceeds 32 MiB".into());
        }
        let text = fs::read_to_string(log).map_err(|e| e.to_string())?;
        (
            events(&text, &log.to_string_lossy()),
            json!({"source":log,"execution_environment":"unverified imported log"}),
            0,
        )
    } else if a.execute {
        capture(a)?
    } else {
        return Err("choose --log PATH or --execute FILE --output-dir NEW_DIR".into());
    };
    if all.is_empty() {
        code = 2;
    }
    if all.iter().any(|e| e["kind"] == "loader_error") && code == 0 {
        code = 1
    }
    let mut comparison = Value::Null;
    if let Some(r) = baseline {
        let expected: std::collections::BTreeSet<_> =
            r.objects.iter().skip(1).map(|o| o.path.clone()).collect();
        let observed: std::collections::BTreeSet<_> = all
            .iter()
            .filter(|e| e["kind"] == "mapped" || e["kind"] == "initialize")
            .filter_map(|e| e["object"].as_str())
            .filter(|p| Path::new(p).is_absolute())
            .map(String::from)
            .collect();
        comparison = json!({"bindings":super::bindings::compare(&r, &all),"predicted_not_observed":expected.difference(&observed).collect::<Vec<_>>(),"observed_not_predicted":observed.difference(&expected).collect::<Vec<_>>(),"note":"Path comparison only; symlink aliases and log omissions may explain differences. Missing observations are not proof of missing loads."});
    }
    Ok((
        json!({"schema_version":1,"kind":if a.execute{"trace_capture"}else{"trace_import"},"status":match code{2=>"incomplete",1=>"runtime_failure",_=>"observed"},"capture":metadata,"events":all,"comparison":comparison,"limitations":["Only the exercised execution path is observed. A successful run does not establish general compatibility.","LD_DEBUG changes runtime overhead; this is not a sandbox. Capture stops the process group at completion/timeout.","A candidate attempt is not a successful load. Initialization order and dependency order are distinct.","Log files are grouped by source/process, not merged into a global timeline. Imported logs cannot establish environment or completeness."]}),
        code,
    ))
}
