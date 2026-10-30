//! Explicit, isolated what-if checks. Never writes into the target rootfs.
use super::{cli::DiagnoseArgs, Diagnostic};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Replacement {
    target: String,
    source: PathBuf,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Overrides {
    schema_version: u32,
    kind: String,
    cwd: Option<String>,
    library_path: Option<Vec<String>>,
    preload: Option<Vec<String>>,
    default_paths: Option<Vec<String>>,
    #[serde(default)]
    replacements: Vec<Replacement>,
}
fn key(d: &Diagnostic) -> (&str, &str, &str, &str) {
    (&d.rule_id, &d.severity, &d.object, &d.message)
}
pub fn run(args: &DiagnoseArgs) -> Result<(Value, i32), String> {
    let path = args.overrides.as_ref().ok_or("--overrides required")?;
    let out = args
        .verification_dir
        .as_ref()
        .ok_or("--verification-dir required")?;
    let mut plan: Overrides = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if plan.schema_version != 1 || plan.kind != "diagnosis_overrides" {
        return Err("expected schema_version 1 diagnosis_overrides".into());
    }
    let before = super::cli::report_for(args)?;
    if plan.cwd.as_ref().is_some_and(|p| !p.starts_with('/'))
        || plan
            .default_paths
            .as_ref()
            .is_some_and(|paths| paths.iter().any(|p| !p.starts_with('/')))
    {
        return Err("override cwd/default paths must be absolute target paths".into());
    }
    let target_path = &before.objects.first().ok_or("empty diagnosis")?.path;
    let mut sources = BTreeMap::new();
    for replacement in &mut plan.replacements {
        let target = Path::new(&replacement.target);
        if !target.is_absolute()
            || target
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            || target == Path::new("/")
        {
            return Err(
                "replacement target must be an absolute file path without parent components".into(),
            );
        }
        replacement.target = target
            .components()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned();
        if &replacement.target == target_path {
            return Err("replacement of the main artifact is not supported".into());
        }
        if before
            .environment
            .locate(&replacement.target)
            .is_ok_and(|p| p.is_dir())
        {
            return Err("replacement target is a directory".into());
        }
        let source = if replacement.source.is_absolute() {
            replacement.source.clone()
        } else {
            path.parent()
                .unwrap_or(Path::new("."))
                .join(&replacement.source)
        };
        replacement.source = fs::canonicalize(source).map_err(|e| e.to_string())?;
        // Validate as ELF and record exactly what is proposed before creating output.
        let elf = super::elf_info::read(&replacement.source)?;
        if sources
            .insert(replacement.target.clone(), elf.sha256)
            .is_some()
        {
            return Err("duplicate replacement target".into());
        }
    }
    fs::create_dir(out).map_err(|e| format!("create new verification directory: {e}"))?;
    let out = fs::canonicalize(out).map_err(|e| e.to_string())?;
    let mut env = before.environment.clone();
    if let Some(v) = &plan.cwd {
        env.cwd = v.clone();
    }
    if let Some(v) = &plan.library_path {
        env.library_path = v.clone();
    }
    if let Some(v) = &plan.preload {
        env.preload = v.clone();
    }
    if let Some(v) = &plan.default_paths {
        env.default_paths = v.clone();
    }
    let mut evidence = vec![];
    for (i, replacement) in plan.replacements.iter_mut().enumerate() {
        let copy = out.join(format!("replacement-{i}.elf"));
        fs::copy(&replacement.source, &copy).map_err(|e| e.to_string())?;
        let copied = super::elf_info::read(&copy)?;
        if sources[&replacement.target] != copied.sha256 {
            return Err("replacement source changed during copy".into());
        }
        env.replacements
            .insert(replacement.target.clone(), copy.clone());
        evidence.push(json!({"target":replacement.target,"source":replacement.source,"copy":copy,"sha256":copied.sha256}));
        replacement.source = copy;
    }
    env.source =
        "explicit repair simulation over unchanged rootfs; replacement files staged separately"
            .into();
    let after = super::diagnose(&args.file, Some(target_path), env)?;
    if after.objects[0].elf.sha256 != before.objects[0].elf.sha256 {
        return Err("main artifact changed during repair verification".into());
    }
    let resolved: Vec<_> = before
        .diagnostics
        .iter()
        .filter(|d| !after.diagnostics.iter().any(|a| key(a) == key(d)))
        .collect();
    let introduced: Vec<_> = after
        .diagnostics
        .iter()
        .filter(|d| !before.diagnostics.iter().any(|b| key(b) == key(d)))
        .collect();
    let remaining: Vec<_> = after
        .diagnostics
        .iter()
        .filter(|d| before.diagnostics.iter().any(|b| key(b) == key(d)))
        .collect();
    super::snapshot::write_new(&out.join("before.json"), &before)?;
    super::snapshot::write_new(&out.join("after.json"), &after)?;
    super::snapshot::write_new(&out.join("overrides.json"), &plan)?;
    let code = after.exit_code();
    let result = json!({"schema_version":1,"kind":"repair_verification","status":after.status,
        "before_status":before.status,"after_status":after.status,"runtime_validation":"not_run",
        "resolved_findings":resolved,"introduced_findings":introduced,"remaining_findings":remaining,
        "output_dir":out,"before_report":out.join("before.json"),"after_report":out.join("after.json"),
        "overrides":plan,"replacement_evidence":evidence,
        "explanation":super::explain::explain(&before,&after),
        "limitations":["Static simulation only; no target files changed and no executable run. No blockers does not prove runtime compatibility.",
        "A disappearing diagnostic may reflect incomplete analysis: inspect after_status and introduced findings.",
        "Saved after.json records a simulated deployment. Use explicit trace in the real deployed environment for runtime evidence; this command does not deploy replacements."]});
    super::snapshot::write_new(&out.join("verification.json"), &result)?;
    Ok((result, code))
}
