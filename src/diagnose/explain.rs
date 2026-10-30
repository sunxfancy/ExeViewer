//! Explain only causal links supported by the saved dependency evidence.
use super::{Edge, Report};
use serde_json::{json, Value};

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
fn hash<'a>(r: &'a Report, path: &str) -> Option<&'a str> {
    r.objects
        .iter()
        .find(|o| o.path == path)
        .map(|o| o.elf.sha256.as_str())
}
fn edge_evidence(r: &Report, edge: &Edge) -> Value {
    json!({"requester":edge.requester,"needed":edge.needed,"selected":edge.selected,
        "sha256":edge.selected.as_deref().and_then(|p|hash(r,p)),"attempts":edge.attempts,"reused":edge.reused})
}
fn first_divergence(a: &Report, ea: &Edge, b: &Report, eb: &Edge) -> Value {
    for i in 0..ea.attempts.len().max(eb.attempts.len()) {
        let va = json!(ea.attempts.get(i));
        let vb = json!(eb.attempts.get(i));
        if va != vb {
            return json!({"kind":"search_attempt","index":i,"a":va,"b":vb});
        }
    }
    if ea.selected != eb.selected {
        return json!({"kind":"selected_path","a":ea.selected,"b":eb.selected});
    }
    let ha = ea.selected.as_deref().and_then(|p| hash(a, p));
    let hb = eb.selected.as_deref().and_then(|p| hash(b, p));
    if ha != hb {
        return json!({"kind":"library_content","a":ha,"b":hb});
    }
    Value::Null
}
pub fn explain(a: &Report, b: &Report) -> Value {
    let mut roots = vec![];
    let mut unexplained = vec![];
    let mut deferred = vec![];
    let mut persistent = vec![];
    let env_a = json!(a.environment);
    let env_b = json!(b.environment);
    let environment_changes: Vec<_> = [
        "cwd",
        "library_path",
        "preload",
        "default_paths",
        "target_machine",
        "isa_level",
        "kernel_release",
        "loader_settings",
    ]
    .iter()
    .filter(|k| env_a[**k] != env_b[**k])
    .map(|k| json!({"field":k,"a":env_a[*k],"b":env_b[*k]}))
    .collect();
    for (index, d) in b
        .diagnostics
        .iter()
        .enumerate()
        .filter(|(_, d)| d.severity == "error" || d.severity == "unknown")
    {
        if a.diagnostics.iter().any(|old| {
            old.rule_id == d.rule_id
                && identity(a, &old.object) == identity(b, &d.object)
                && old.message == d.message
                && old.severity == d.severity
        }) {
            persistent.push(json!({"diagnostic_index":index,"diagnostic":d}));
            continue;
        }
        if d.rule_id == "SYMBOL_UNRESOLVED" && d.severity == "unknown" {
            deferred.push(json!({"diagnostic_index":index,"diagnostic":d,"reason":"Symbol absence is not conclusive while the dependency/scope model is incomplete; not an independent proven root cause."}));
            continue;
        }
        let related: Vec<_> = b
            .dependencies
            .iter()
            .filter(|edge| {
                edge.requester == d.object
                    && match d.rule_id.as_str() {
                        "LIBRARY_NOT_FOUND" => {
                            edge.selected.is_none() && d.chain.last() == Some(&edge.needed)
                        }
                        "VERSION_MISSING" => d
                            .evidence
                            .iter()
                            .any(|e| e.starts_with(&format!("DT_VERNEED: {}@", edge.needed))),
                        _ => false,
                    }
            })
            .collect();
        if related.len() == 1 {
            let eb = related[0];
            let before: Vec<_> = a
                .dependencies
                .iter()
                .filter(|ea| {
                    ea.needed == eb.needed
                        && identity(a, &ea.requester) == identity(b, &eb.requester)
                })
                .collect();
            if before.len() == 1 {
                let ea = before[0];
                let divergence = first_divergence(a, ea, b, eb);
                if ea.selected.is_some()
                    && !divergence.is_null()
                    && hash(a, &ea.requester) == hash(b, &eb.requester)
                    && !a.diagnostics.iter().any(|old| {
                        old.rule_id == d.rule_id
                            && identity(a, &old.object) == identity(b, &d.object)
                            && old.evidence.first() == d.evidence.first()
                    })
                {
                    roots.push(json!({"diagnostic_index":index,"rule_id":d.rule_id,"certainty":"static_causal_link",
                        "chain":d.chain,"first_divergence":divergence,"evidence_a":edge_evidence(a,ea),"evidence_b":edge_evidence(b,eb),
                        "consequence":d.message,"requirement_evidence":d.evidence,"remediation":d.remediation,
                        "explanation":if d.rule_id=="LIBRARY_NOT_FOUND" {"The same requester resolved this dependency in A; B's recorded search cannot resolve it."} else {"The same requester requires this version; B selects different library content/path that lacks the required version."}}));
                    continue;
                }
            }
        }
        if d.rule_id == "SYMBOL_UNRESOLVED" && d.severity == "error" {
            let missing = b.bindings.iter().find(|p| {
                p.requester == d.object
                    && !p.weak
                    && p.provider.is_none()
                    && d.message
                        == format!(
                            "No startup-scope definition of {}{}",
                            p.symbol,
                            p.version
                                .as_ref()
                                .map(|v| format!("@{v}"))
                                .unwrap_or_default()
                        )
            });
            if let Some(pb) = missing {
                let old: Vec<_> = a
                    .bindings
                    .iter()
                    .filter(|p| {
                        identity(a, &p.requester) == identity(b, &pb.requester)
                            && p.symbol == pb.symbol
                            && p.version == pb.version
                            && p.provider.is_some()
                            && p.certainty == "predicted"
                    })
                    .collect();
                if old.len() == 1 && hash(a, &old[0].requester) == hash(b, &pb.requester) {
                    let ea = a
                        .dependencies
                        .iter()
                        .find(|edge| edge.selected == old[0].provider);
                    let eb = ea.and_then(|ea| {
                        b.dependencies.iter().find(|edge| {
                            edge.needed == ea.needed
                                && identity(a, &ea.requester) == identity(b, &edge.requester)
                        })
                    });
                    let divergence = ea.zip(eb).map(|(ea, eb)| first_divergence(a, ea, b, eb));
                    roots.push(json!({"diagnostic_index":index,"rule_id":d.rule_id,"certainty":"static_causal_link",
                        "first_divergence":divergence,"evidence_a":ea.map(|e|edge_evidence(a,e)),"evidence_b":eb.map(|e|edge_evidence(b,e)),
                        "chain":d.chain,"binding_a":old[0],"binding_b":pb,"consequence":d.message,"remediation":d.remediation,
                        "explanation":"A has a modeled provider for this unchanged request; B's complete modeled startup scope has no matching definition. This does not establish when a lazy binding executes."}));
                    continue;
                }
            }
        }
        unexplained.push(json!({"diagnostic_index":index,"diagnostic":d,"certainty":"unknown",
            "reason":"Insufficient unique, unchanged-requester evidence to attribute this finding to an A/B dependency divergence."}));
    }
    json!({"root_causes":roots,"unexplained":unexplained,"deferred":deferred,"persistent_findings":persistent,
        "environment_changes":environment_changes,"limitations":["Causal links concern the modeled static startup requirements, not proof that A executes successfully.",
        "Environment changes are context, not automatically causal. Ambiguous identities and unsupported rules remain unexplained.",
        "First divergence is per dependency search record, not a global execution timestamp."]})
}
