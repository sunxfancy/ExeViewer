pub mod cli;
pub mod elf_info;
pub mod environment;
pub mod trace;
pub mod view;
pub mod snapshot;
use elf_info::ElfInfo;
use environment::{CacheEntry, Environment};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub rule_id: String,
    pub severity: String,
    pub certainty: String,
    pub object: String,
    pub chain: Vec<String>,
    pub message: String,
    pub evidence: Vec<String>,
    pub remediation: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub path: String,
    pub source: String,
    pub outcome: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub requester: String,
    pub needed: String,
    pub selected: Option<String>,
    pub reused: bool,
    pub attempts: Vec<Attempt>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Object {
    pub path: String,
    pub chain: Vec<String>,
    pub elf: ElfInfo,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub kind: String,
    pub artifact: String,
    pub environment: Environment,
    pub status: String,
    pub scope: Vec<String>,
    pub limitations: Vec<String>,
    pub objects: Vec<Object>,
    pub dependencies: Vec<Edge>,
    pub diagnostics: Vec<Diagnostic>,
}
impl Report {
    pub fn exit_code(&self) -> i32 {
        if self.diagnostics.iter().any(|d| d.severity == "unknown") {
            2
        } else if self.diagnostics.iter().any(|d| d.severity == "error") {
            1
        } else {
            0
        }
    }
    fn issue(
        &mut self,
        rule: &str,
        severity: &str,
        obj: &str,
        chain: &[String],
        message: String,
        evidence: Vec<String>,
        fix: &str,
    ) {
        self.diagnostics.push(Diagnostic {
            rule_id: rule.into(),
            severity: severity.into(),
            certainty: if severity == "unknown" {
                "unknown"
            } else {
                "static"
            }
            .into(),
            object: obj.into(),
            chain: chain.to_vec(),
            message,
            evidence,
            remediation: fix.into(),
        });
    }
    pub fn text(&self) -> String {
        use std::fmt::Write;
        let mut s=format!("ExeViewer compatibility diagnosis\nStatus: {}\nArtifact: {:?}\nTarget root: {:?}\nScope: {}\n",self.status,self.artifact,self.environment.rootfs,self.scope.join(", "));
        for d in &self.diagnostics {
            writeln!(
                s,
                "\n[{}] {}: {:?}\n  Object: {:?}\n  Chain: {:?}\n  Evidence: {:?}\n  Action: {:?}",
                d.severity, d.rule_id, d.message, d.object, d.chain, d.evidence, d.remediation
            )
            .unwrap();
        }
        s.push_str("\nDependency search evidence:\n");
        for e in &self.dependencies {
            writeln!(
                s,
                "  {:?} -> {:?} => {:?}{}",
                e.requester,
                e.needed,
                e.selected,
                if e.reused { " (reused)" } else { "" }
            )
            .unwrap();
            for a in &e.attempts {
                writeln!(s, "    [{}] {:?}: {:?}", a.source, a.path, a.outcome).unwrap();
            }
        }
        s.push_str("\nLimits (no guarantee of successful execution):\n");
        for l in &self.limitations {
            writeln!(s, "  {l}").unwrap();
        }
        s
    }
}
fn expand(value: &str, origin: &str) -> Result<String, String> {
    let s = value
        .replace("${ORIGIN}", origin)
        .replace("$ORIGIN", origin);
    if s.contains('$') {
        return Err(format!(
            "target-specific token expansion unavailable: {value}"
        ));
    }
    Ok(s)
}
fn paths(value: &str, origin: &str, source: &str) -> Result<Vec<(String, String)>, String> {
    value
        .split([':', ';'])
        .map(|p| Ok((expand(p, origin)?, source.into())))
        .collect()
}
fn origin(path: &str) -> String {
    Path::new(path)
        .parent()
        .unwrap_or(Path::new("/"))
        .to_string_lossy()
        .into()
}

pub fn diagnose(
    input: &Path,
    target_path: Option<&str>,
    env: Environment,
) -> Result<Report, String> {
    let main = elf_info::read(input)?;
    let logical = target_path.map(|v| env.absolute(v)).unwrap_or_else(|| {
        let full = fs::canonicalize(input).unwrap_or_else(|_| input.to_path_buf());
        if full.starts_with(&env.rootfs) {
            env.virtual_path(&full)
        } else {
            format!(
                "/{}",
                input.file_name().unwrap_or_default().to_string_lossy()
            )
        }
    });
    let mut report=Report{schema_version:1,kind:"diagnosis".into(),artifact:input.to_string_lossy().into(),environment:env.clone(),status:String::new(),scope:vec!["Linux ELF64 x86-64 glibc startup dependency and symbol requirements".into()],limitations:vec!["Static startup model; does not prove program execution, ABI semantics, CPU instruction or kernel compatibility.".into(),"dlopen, separate namespaces, IFUNC execution and lazy-binding execution paths require runtime observation.".into(),"Default directories are explicit model inputs; use --default-path to match the target loader build. $LIB/$PLATFORM and CPU hwcaps require target-specific evidence.".into()],objects:vec![Object{path:logical.clone(),chain:vec![logical.clone()],elf:main.clone()}],dependencies:vec![],diagnostics:vec![]};
    if target_path.is_none()
        && !fs::canonicalize(input)
            .map_err(|e| e.to_string())?
            .starts_with(&env.rootfs)
    {
        report.issue(
            "INSTALL_PATH_UNKNOWN",
            "unknown",
            &logical,
            &[logical.clone()],
            "Input is outside rootfs; its installed path is unknown".into(),
            vec![format!("provisional path: {logical}")],
            "Set --target-path to the absolute installed path inside the target root.",
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(input)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o6000
            != 0
        {
            report.issue("SECURE_EXECUTION_UNKNOWN", "unknown", &logical, &[logical.clone()],
                "setuid/setgid artifact: secure-execution environment rules are not modeled".into(), vec![],
                "Validate under the actual target credentials; loader environment variables may be ignored.");
        }
    }
    if main.machine != env.target_machine {
        report.issue(
            "ARCH_MISMATCH",
            "error",
            &logical,
            &[logical.clone()],
            format!(
                "ELF machine {} does not match target {}",
                main.machine, env.target_machine
            ),
            vec!["ELF e_machine".into()],
            "Rebuild for the target architecture.",
        );
    }
    if main.machine != 62 || env.target_machine != 62 {
        report.issue(
            "UNSUPPORTED_ARCH",
            "unknown",
            &logical,
            &[logical.clone()],
            "Resolver currently supports x86-64 only".into(),
            vec![],
            "Use a supported target or inspect with its native tooling.",
        );
        return finish(report);
    }
    if ![2, 3].contains(&main.elf_type) {
        report.issue(
            "UNSUPPORTED_TYPE",
            "unknown",
            &logical,
            &[logical.clone()],
            "Expected executable or shared object".into(),
            vec![],
            "Provide a linked exe/so.",
        );
        return finish(report);
    }
    if !main.dynamic {
        report.limitations.push("No PT_DYNAMIC: shared-library startup checks are not applicable; static runtime behavior is unassessed.".into());
        return finish(report);
    }
    if let Some(interp) = &main.interpreter {
        if !interp.contains("ld-linux-x86-64.so.2") {
            report.issue(
                "UNSUPPORTED_LOADER",
                "unknown",
                &logical,
                &[logical.clone()],
                format!("Unsupported interpreter {interp}"),
                vec!["PT_INTERP".into()],
                "Use the matching loader's rules; this model targets glibc.",
            );
            return finish(report);
        }
        match env.locate(interp).and_then(|p| fs::metadata(p)) {
            Ok(m) if m.is_file() => {}
            Ok(_) | Err(_) => report.issue(
                "INTERPRETER_MISSING",
                "error",
                &logical,
                &[logical.clone()],
                format!("Target interpreter {interp} is unavailable"),
                vec!["PT_INTERP".into()],
                "Deploy the matching runtime or rebuild with the target toolchain/sysroot.",
            ),
        }
    } else if main.dynamic {
        report.issue("HOST_SCOPE_UNKNOWN","unknown",&logical,&[logical.clone()],"Shared object has no executable host; symbol scope and loader family are assumed glibc".into(),vec!["PT_INTERP absent".into()],"Diagnose its host executable for a definitive startup scope.");
    }
    let cache = match environment::cache(&env) {
        Ok(c) => c,
        Err(e) => {
            report.issue(
                "CACHE_UNKNOWN",
                "unknown",
                &logical,
                &[logical.clone()],
                e,
                vec!["/etc/ld.so.cache".into()],
                "Provide a supported glibc cache or verify using target loader traces.",
            );
            vec![]
        }
    };
    let mut queue = VecDeque::from([(0usize, Vec::<(String, String)>::new())]);
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(n) = &main.soname {
        names.insert(n.clone(), 0);
    }
    let mut files: BTreeMap<PathBuf, usize> = BTreeMap::new();
    files.insert(fs::canonicalize(input).map_err(|e| e.to_string())?, 0);
    // Load interpreter as an object for symbol/version resolution but traverse its dependencies last.
    let mut interp_index = None;
    if let Some(interp) = &main.interpreter {
        if let Ok(p) = env.locate(interp) {
            match elf_info::read(&p) {
                Ok(i) => {
                    if i.machine != 62 {
                        report.issue(
                            "INTERPRETER_ARCH",
                            "error",
                            &logical,
                            &[logical.clone()],
                            "Interpreter architecture mismatch".into(),
                            vec![interp.clone()],
                            "Install a matching x86-64 runtime.",
                        );
                    }
                    let idx = report.objects.len();
                    if let Some(n) = &i.soname {
                        names.insert(n.clone(), idx);
                    }
                    names.insert(interp.clone(), idx);
                    files.insert(p, idx);
                    report.objects.push(Object {
                        path: interp.clone(),
                        chain: vec![logical.clone(), interp.clone()],
                        elf: i,
                    });
                    interp_index = Some(idx);
                }
                Err(e) => report.issue(
                    "INTERPRETER_INVALID",
                    "unknown",
                    &logical,
                    &[logical.clone()],
                    e,
                    vec![interp.clone()],
                    "Inspect the target interpreter file.",
                ),
            }
        }
    }
    let mut preload = env.preload.clone();
    match env.locate("/etc/ld.so.preload") {
        Ok(p) => match fs::read_to_string(p) {
            Ok(s) => {
                for line in s.lines() {
                    preload.extend(
                        line.split('#')
                            .next()
                            .unwrap_or("")
                            .split_whitespace()
                            .map(String::from),
                    );
                }
            }
            Err(e) => report.issue(
                "PRELOAD_UNKNOWN",
                "unknown",
                &logical,
                &[logical.clone()],
                e.to_string(),
                vec!["/etc/ld.so.preload".into()],
                "Make target preload configuration readable.",
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => report.issue(
            "PRELOAD_UNKNOWN",
            "unknown",
            &logical,
            &[logical.clone()],
            e.to_string(),
            vec![],
            "Check target preload configuration.",
        ),
    }
    // Preloads precede DT_NEEDED. Treat their own dependencies using the main loader context.
    let preload_paths = if main.runpath.is_none() {
        main.rpath
            .as_ref()
            .map(|p| paths(p, &origin(&logical), &format!("RPATH of {logical}")))
            .transpose()
    } else {
        Ok(None)
    };
    let preload_paths = match preload_paths {
        Ok(p) => p.unwrap_or_default(),
        Err(e) => {
            report.issue(
                "TOKEN_UNKNOWN",
                "unknown",
                &logical,
                &[logical.clone()],
                e,
                vec![],
                "Resolve target RPATH tokens.",
            );
            vec![]
        }
    };
    for name in preload {
        load(
            &mut report,
            0,
            &name,
            &preload_paths,
            &cache,
            &mut names,
            &mut files,
            &mut queue,
            true,
        );
    }
    while let Some((idx, inherited)) = queue.pop_front() {
        if report.objects.len() > 4096 {
            report.issue(
                "RESOURCE_LIMIT",
                "unknown",
                &logical,
                &[logical.clone()],
                "Dependency graph exceeds 4096 objects".into(),
                vec![],
                "Analyze a smaller dependency graph.",
            );
            break;
        }
        let obj = report.objects[idx].clone();
        let mut next = inherited.clone();
        if obj.elf.runpath.is_none() {
            if let Some(rpath) = &obj.elf.rpath {
                match paths(rpath, &origin(&obj.path), &format!("RPATH of {}", obj.path)) {
                    Ok(p) => {
                        let mut p = p;
                        p.extend(next);
                        next = p;
                    }
                    Err(e) => report.issue(
                        "TOKEN_UNKNOWN",
                        "unknown",
                        &obj.path,
                        &obj.chain,
                        e,
                        vec![rpath.clone()],
                        "Supply target-specific token values in deployment paths.",
                    ),
                }
            }
        }
        for name in &obj.elf.needed {
            load(
                &mut report,
                idx,
                name,
                &next,
                &cache,
                &mut names,
                &mut files,
                &mut queue,
                false,
            );
        }
    }
    if let Some(i) = interp_index {
        if !report.objects[i].elf.needed.is_empty() {
            report.issue(
                "LOADER_DEPENDENCIES",
                "unknown",
                &logical,
                &[logical.clone()],
                "Interpreter declares dependencies not traversed by this model".into(),
                report.objects[i].elf.needed.clone(),
                "Inspect the target loader implementation.",
            );
        }
    }
    check_symbols(&mut report, &names);
    finish(report)
}
fn finish(mut r: Report) -> Result<Report, String> {
    for o in r.objects.clone() {
        for note in &o.elf.notes {
            r.issue(
                "UNMODELED_ELF_FEATURE",
                "unknown",
                &o.path,
                &o.chain,
                note.clone(),
                vec![],
                "Validate this object with a target loader trace.",
            );
        }
        if let Some(required) = o.elf.required_isa_level {
            match r.environment.isa_level {
                Some(target) if target < required => r.issue("ISA_REQUIREMENT", "error", &o.path, &o.chain,
                    format!("Requires x86-64-v{required}; target specifies v{target}"), vec!["GNU_PROPERTY_X86_ISA_1_NEEDED".into()],
                    "Rebuild for the target CPU baseline or use a CPU that supports the declared ISA."),
                None if required > 1 => r.issue("ISA_UNKNOWN", "unknown", &o.path, &o.chain,
                    format!("Requires x86-64-v{required}; target ISA is unknown"), vec!["GNU_PROPERTY_X86_ISA_1_NEEDED".into()], "Provide --isa-level from target CPU evidence."),
                _ => {}
            }
        }
        if let Some(required) = o.elf.minimum_kernel {
            let target = r.environment.kernel_release.as_deref().and_then(|s| {
                let mut fields = s.split(['.', '-']);
                Some([
                    fields.next()?.parse::<u32>().ok()?,
                    fields.next()?.parse::<u32>().ok()?,
                    fields.next().unwrap_or("0").parse::<u32>().ok()?,
                ])
            });
            match target {
                Some(target) if target < required => r.issue(
                    "KERNEL_REQUIREMENT",
                    "error",
                    &o.path,
                    &o.chain,
                    format!("Declared minimum kernel {required:?} exceeds target {target:?}"),
                    vec!["GNU ABI-tag note".into()],
                    "Use a supported kernel or rebuild against an appropriate runtime baseline.",
                ),
                None => r.issue(
                    "KERNEL_UNKNOWN",
                    "unknown",
                    &o.path,
                    &o.chain,
                    format!("Declared minimum kernel {required:?}; target release is unknown"),
                    vec!["GNU ABI-tag note".into()],
                    "Provide --kernel-release from the target environment.",
                ),
                _ => {}
            }
        }
    }
    r.status = match r.exit_code() {
        2 => "incomplete",
        1 => "blocked",
        _ => "no_blockers_in_checked_scope",
    }
    .into();
    Ok(r)
}

#[allow(clippy::too_many_arguments)]
fn load(
    report: &mut Report,
    idx: usize,
    name: &str,
    inherited: &[(String, String)],
    cache: &[CacheEntry],
    names: &mut BTreeMap<String, usize>,
    files: &mut BTreeMap<PathBuf, usize>,
    queue: &mut VecDeque<(usize, Vec<(String, String)>)>,
    preload: bool,
) {
    let env = report.environment.clone();
    let obj = report.objects[idx].clone();
    let mut edge = Edge {
        requester: obj.path.clone(),
        needed: name.into(),
        selected: None,
        reused: false,
        attempts: vec![],
    };
    if let Some(&existing) = names.get(name) {
        edge.selected = Some(report.objects[existing].path.clone());
        edge.reused = true;
        report.dependencies.push(edge);
        return;
    }
    let mut candidates = vec![];
    let mut uncertain = false;
    let expanded = match expand(name, &origin(&obj.path)) {
        Ok(n) => n,
        Err(e) => {
            report.issue(
                "TOKEN_UNKNOWN",
                "unknown",
                &obj.path,
                &obj.chain,
                e,
                vec![name.into()],
                "Inspect target token expansion.",
            );
            report.dependencies.push(edge);
            return;
        }
    };
    if expanded.contains('/') {
        candidates.push((env.absolute(&expanded), "DT_NEEDED path".into()));
    } else {
        if obj.elf.runpath.is_none() {
            for (p, s) in inherited {
                let p = if p.is_empty() { &env.cwd } else { p };
                candidates.push((env.absolute(&format!("{p}/{expanded}")), s.clone()));
            }
        }
        for p in &env.library_path {
            match expand(p, &origin(&report.objects[0].path)) {
                Ok(p) => candidates.push((
                    env.absolute(&format!(
                        "{}/{expanded}",
                        if p.is_empty() { env.cwd.clone() } else { p }
                    )),
                    "library-path".into(),
                )),
                Err(e) => {
                    uncertain = true;
                    report.issue(
                        "TOKEN_UNKNOWN",
                        "unknown",
                        &obj.path,
                        &obj.chain,
                        e,
                        vec![p.clone()],
                        "Resolve target library-path tokens.",
                    );
                }
            }
        }
        if let Some(p) = &obj.elf.runpath {
            match paths(p, &origin(&obj.path), &format!("RUNPATH of {}", obj.path)) {
                Ok(v) => {
                    for (p, s) in v {
                        let p = if p.is_empty() { env.cwd.clone() } else { p };
                        candidates.push((env.absolute(&format!("{p}/{expanded}")), s));
                    }
                }
                Err(e) => {
                    uncertain = true;
                    report.issue(
                        "TOKEN_UNKNOWN",
                        "unknown",
                        &obj.path,
                        &obj.chain,
                        e,
                        vec![p.clone()],
                        "Resolve target RUNPATH tokens.",
                    );
                }
            }
        }
        for c in cache.iter().filter(|e| e.name == expanded) {
            if obj.elf.nodeflib
                && env
                    .default_paths
                    .iter()
                    .any(|p| Path::new(&c.path).starts_with(p))
            {
                continue;
            }
            if c.hwcap != 0 {
                uncertain = true;
                report.issue(
                    "HWCAPS_UNKNOWN",
                    "unknown",
                    &obj.path,
                    &obj.chain,
                    format!("CPU-dependent cache candidate {} cannot be ranked", c.path),
                    vec!["ld.so.cache hwcap".into()],
                    "Capture target CPU capabilities and validate the target loader trace.",
                );
                continue;
            }
            candidates.push((c.path.clone(), "ld.so.cache".into()));
        }
        if !obj.elf.nodeflib {
            for p in &env.default_paths {
                candidates.push((
                    format!("{}/{expanded}", p.trim_end_matches('/')),
                    "default-path".into(),
                ));
            }
        }
    }
    for (path, source) in candidates {
        let parent = Path::new(&path).parent().unwrap_or(Path::new("/"));
        if env
            .locate(&parent.join("glibc-hwcaps").to_string_lossy())
            .is_ok()
        {
            uncertain = true;
            report.issue(
                "HWCAPS_UNKNOWN",
                "unknown",
                &obj.path,
                &obj.chain,
                "glibc-hwcaps directory requires target CPU selection".into(),
                vec![path.clone()],
                "Validate with target loader trace.",
            );
        }
        let resolved = match env.locate(&path) {
            Ok(p) => p,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    uncertain = true;
                }
                edge.attempts.push(Attempt {
                    path,
                    source,
                    outcome: e.to_string(),
                });
                continue;
            }
        };
        let mut existing_file = files.get(&resolved).copied();
        #[cfg(unix)]
        if existing_file.is_none() {
            use std::os::unix::fs::MetadataExt;
            if let Ok(candidate) = fs::metadata(&resolved) {
                existing_file = files.iter().find_map(|(path, &index)| {
                    fs::metadata(path)
                        .ok()
                        .filter(|m| m.dev() == candidate.dev() && m.ino() == candidate.ino())
                        .map(|_| index)
                });
            }
        }
        if let Some(existing) = existing_file {
            edge.selected = Some(report.objects[existing].path.clone());
            edge.reused = true;
            names.insert(name.into(), existing);
            edge.attempts.push(Attempt {
                path,
                source,
                outcome: "reuse loaded file".into(),
            });
            break;
        }
        let info = match elf_info::read(&resolved) {
            Ok(i) => i,
            Err(e) => {
                edge.attempts.push(Attempt {
                    path: path.clone(),
                    source,
                    outcome: e.clone(),
                });
                uncertain = true;
                report.issue("CANDIDATE_INVALID","unknown",&obj.path,&obj.chain,e,vec![path],"Repair or inspect the selected library candidate; loader behavior may stop at this file.");
                break;
            }
        };
        if info.machine != env.target_machine {
            edge.attempts.push(Attempt {
                path,
                source,
                outcome: format!("rejected: machine {}", info.machine),
            });
            continue;
        }
        if info.elf_type != 3 {
            uncertain = true;
            report.issue(
                "LIBRARY_TYPE",
                "unknown",
                &obj.path,
                &obj.chain,
                "Dependency candidate is not ET_DYN".into(),
                vec![path.clone()],
                "Deploy a valid shared object.",
            );
            edge.attempts.push(Attempt {
                path,
                source,
                outcome: "invalid shared object type".into(),
            });
            break;
        }
        let n = report.objects.len();
        let mut chain = obj.chain.clone();
        chain.push(path.clone());
        names.insert(name.into(), n);
        if let Some(soname) = &info.soname {
            names.entry(soname.clone()).or_insert(n);
        }
        files.insert(resolved, n);
        edge.selected = Some(path.clone());
        edge.attempts.push(Attempt {
            path: path.clone(),
            source,
            outcome: if preload {
                "selected preload"
            } else {
                "selected"
            }
            .into(),
        });
        report.objects.push(Object {
            path,
            chain,
            elf: info,
        });
        queue.push_back((n, inherited.to_vec()));
        break;
    }
    if edge.selected.is_none() {
        let mut chain = obj.chain.clone();
        chain.push(name.into());
        report.issue(if preload { "PRELOAD_NOT_FOUND" } else { "LIBRARY_NOT_FOUND" },if uncertain { "unknown" } else if preload { "warning" } else { "error" },&obj.path,&chain,format!("Cannot resolve {name}"),edge.attempts.iter().map(|a|format!("{}: {} ({})",a.path,a.outcome,a.source)).collect(),"Deploy the missing target dependency or fix its RPATH/RUNPATH/library path, then rerun diagnosis.");
    }
    report.dependencies.push(edge);
}
fn check_symbols(r: &mut Report, names: &BTreeMap<String, usize>) {
    let objects = r.objects.clone();
    let mut exports: BTreeMap<&str, Vec<&elf_info::Symbol>> = BTreeMap::new();
    for o in &objects {
        for s in &o.elf.symbols {
            if s.defined {
                exports.entry(&s.name).or_default().push(s);
            }
        }
    }
    for o in &objects {
        for v in &o.elf.versions_needed {
            if let Some(&provider) = names.get(&v.library) {
                let p = &objects[provider];
                if p.elf.versions_defined.is_empty() {
                    r.issue("VERSION_METADATA_ABSENT", "warning", &o.path, &o.chain,
                        format!("{} has no version definitions for requested {}; glibc may allow an unversioned provider",p.path,v.version),
                        vec![format!("DT_VERNEED: {}@{}",v.library,v.version)],
                        "Verify the provider ABI and actual binding; missing version metadata alone is not proof of a load failure.");
                } else if !p.elf.versions_defined.contains(&v.version) {
                    r.issue("VERSION_MISSING",if v.weak{"warning"}else{"error"},&o.path,&o.chain,format!("{} requires {} from {}, which does not define it",o.path,v.version,p.path),vec![format!("DT_VERNEED: {}@{}",v.library,v.version),format!("provider SHA-256: {}",p.elf.sha256)],"Rebuild against a compatible target sysroot or deploy a runtime providing the required version. Do not substitute libc with an arbitrary symlink.");
                }
            }
        }
        for s in &o.elf.symbols {
            if s.defined || s.weak {
                continue;
            }
            let found = exports.get(s.name.as_str()).is_some_and(|items| {
                items.iter().any(|p| match &s.version {
                    Some(v) => p.version.as_ref() == Some(v) || p.version.is_none(),
                    None => p.version.is_none() || p.default_version,
                })
            });
            if !found {
                let partial = r.objects.iter().any(|o| !o.elf.notes.is_empty())
                    || r.diagnostics
                        .iter()
                        .any(|d| d.severity == "unknown" || d.rule_id == "LIBRARY_NOT_FOUND");
                r.issue("SYMBOL_UNRESOLVED",if partial{"unknown"}else{"error"},&o.path,&o.chain,format!("No startup-scope definition of {}{}",s.name,s.version.as_ref().map(|v|format!("@{v}")).unwrap_or_default()),vec!["strong undefined dynamic symbol".into()],"Check the selected libraries and build ABI; lazy or plugin-provided bindings require runtime verification.");
            }
        }
    }
}
