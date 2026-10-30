//! Startup global-scope predictions, kept separate from runtime observations.
use super::Report;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub object: String,
    pub version: Option<String>,
    pub matches: bool,
    pub weak: bool,
    pub symbol_type: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub requester: String,
    pub symbol: String,
    pub version: Option<String>,
    pub weak: bool,
    pub scope: String,
    pub candidates: Vec<Candidate>,
    pub provider: Option<String>,
    pub shadowed: Vec<String>,
    pub certainty: String,
    pub reason: String,
}
pub fn predict(r: &Report) -> Vec<Binding> {
    let Some(main) = r.objects.first() else {
        return vec![];
    };
    // Main, explicit preloads, direct dependencies, then breadth-first children.
    // The interpreter was parsed early but is not first in global lookup order.
    let mut queue = VecDeque::from([main.path.clone()]);
    let mut seen = BTreeSet::new();
    let mut order = vec![];
    while let Some(path) = queue.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Some(object) = r.objects.iter().find(|o| o.path == path) {
            order.push(object);
        }
        for edge in r.dependencies.iter().filter(|e| e.requester == path) {
            if let Some(selected) = &edge.selected {
                queue.push_back(selected.clone());
            }
        }
    }
    for object in &r.objects {
        if seen.insert(object.path.clone()) {
            order.push(object);
        }
    }
    let incomplete = r
        .diagnostics
        .iter()
        .any(|d| d.severity == "unknown" || d.rule_id == "LIBRARY_NOT_FOUND")
        || r.objects.iter().any(|o| !o.elf.notes.is_empty());
    let mut exports = BTreeMap::<&str, Vec<(&str, &super::elf_info::Symbol)>>::new();
    for object in order {
        for symbol in object.elf.symbols.iter().filter(|s| s.defined) {
            exports
                .entry(&symbol.name)
                .or_default()
                .push((&object.path, symbol));
        }
    }
    let mut result = vec![];
    for object in &r.objects {
        for symbol in object.elf.symbols.iter().filter(|s| !s.defined) {
            let candidates: Vec<_> = exports
                .get(symbol.name.as_str())
                .into_iter()
                .flatten()
                .map(|(path, s)| {
                    let matches = match &symbol.version {
                        Some(version) => s.version.as_ref() == Some(version) || s.version.is_none(),
                        None => s.version.is_none() || s.default_version,
                    };
                    Candidate {
                        object: (*path).into(),
                        version: s.version.clone(),
                        matches,
                        weak: s.weak,
                        symbol_type: s.symbol_type,
                    }
                })
                .collect();
            let matching: Vec<_> = candidates
                .iter()
                .filter(|c| c.matches)
                .map(|c| c.object.clone())
                .collect();
            result.push(Binding {
                requester: object.path.clone(), symbol: symbol.name.clone(), version: symbol.version.clone(), weak: symbol.weak,
                scope: "startup global namespace; undefined dynamic symbols only".into(),
                provider: if incomplete { None } else { matching.first().cloned() },
                shadowed: if incomplete { vec![] } else { matching.into_iter().skip(1).collect() },
                candidates, certainty: if incomplete { "unknown" } else { "predicted" }.into(),
                reason: if incomplete { "Incomplete dependency graph or unsupported ELF/scope rules" }
                    else { "First matching definition in modeled startup order; default glibc weak-symbol semantics. IFUNC implementation addresses and defined-symbol interposition are not predicted." }.into(),
            });
        }
    }
    result
}
/// Paths are compared literally. A mismatch is evidence to investigate, not proof of failure.
pub fn compare(r: &Report, events: &[Value]) -> Value {
    let mut checks = vec![];
    let mut observed = BTreeSet::new();
    for event in events {
        let Some(binding) = event.get("binding") else {
            continue;
        };
        let requester = binding["requester"].as_str().unwrap_or("");
        let symbol = binding["symbol"].as_str().unwrap_or("");
        let version = binding["version"].as_str();
        let prediction = r.bindings.iter().find(|p| {
            p.requester == requester && p.symbol == symbol && p.version.as_deref() == version
        });
        let namespace_supported =
            binding["requester_namespace"] == "0" && binding["provider_namespace"] == "0";
        let status = match prediction {
            Some(p) if namespace_supported && p.certainty == "predicted" => {
                observed.insert((
                    requester.to_string(),
                    symbol.to_string(),
                    version.map(String::from),
                ));
                if p.provider.as_deref() == binding["provider"].as_str() {
                    "match"
                } else {
                    "mismatch"
                }
            }
            _ => "unmodeled",
        };
        checks.push(json!({"status":status,"prediction":prediction,"observed":binding,
            "source":event["source"],"line":event["line"],"pid":event["pid"],"evidence":event["evidence"]}));
    }
    let not_observed: Vec<_> = r
        .bindings
        .iter()
        .filter(|p| !observed.contains(&(p.requester.clone(), p.symbol.clone(), p.version.clone())))
        .map(|p| json!({"requester":p.requester,"symbol":p.symbol,"version":p.version}))
        .collect();
    json!({"checks":checks,"not_observed":not_observed,
        "limitations":["Literal path matching; aliases may produce unmodeled/mismatching results. Imported trace identity is unverified.",
        "Unobserved symbols may be lazy or unused. A mismatch does not establish runtime failure. Only global namespace 0 and undefined symbols are predicted."]})
}
