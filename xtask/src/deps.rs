//! The no-I/O allow-list.
//!
//! Two checks for every library crate (every workspace member but xtask):
//!
//! 1. Its resolved normal dependencies, transitively, are all on the
//!    allow-list below. harness-core may depend directly on serde only.
//! 2. Its `src/` never names the standard library's file, network, process,
//!    environment or OS modules. All I/O goes through traits the engine
//!    implements.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

/// Outside crates any library crate may reach, directly or transitively.
const ALLOWED: &[&str] = &[
    "serde",
    "serde_core",
    "serde_derive",
    "serde_json",
    "itoa",
    "ryu",
    "zmij",
    "memchr",
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
];

/// Crates with a tighter rule: their only permitted direct dependencies.
const DIRECT_ONLY: &[(&str, &[&str])] = &[("harness-core", &["serde"])];

/// Paths library code may not name.
const FORBIDDEN_PATHS: &[&str] = &[
    "std::fs",
    "std::net",
    "std::process",
    "std::env",
    "std::os",
    "std::io::stdin",
    "std::io::stdout",
    "std::io::stderr",
];

/// Crates exempt from both checks.
const EXEMPT: &[&str] = &["xtask"];

pub fn run() -> Result<(), String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--all-features",
        ])
        .current_dir(crate::workspace_root())
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let meta: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("metadata json: {e}"))?;

    let members: BTreeSet<String> = meta["workspace_members"]
        .as_array()
        .ok_or("no workspace_members")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();

    let mut names = BTreeMap::new();
    let mut dirs = BTreeMap::new();
    for pkg in meta["packages"].as_array().ok_or("no packages")? {
        let id = pkg["id"].as_str().unwrap_or_default().to_owned();
        names.insert(
            id.clone(),
            pkg["name"].as_str().unwrap_or_default().to_owned(),
        );
        if let Some(path) = pkg["manifest_path"].as_str() {
            let dir = std::path::Path::new(path).parent().map(|p| p.to_path_buf());
            dirs.insert(id, dir);
        }
    }

    // Normal (non-dev, non-build) edges of the resolved graph.
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in meta["resolve"]["nodes"]
        .as_array()
        .ok_or("no resolve graph")?
    {
        let id = node["id"].as_str().unwrap_or_default().to_owned();
        let mut out = Vec::new();
        for dep in node["deps"].as_array().into_iter().flatten() {
            let normal = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if normal {
                out.push(dep["pkg"].as_str().unwrap_or_default().to_owned());
            }
        }
        edges.insert(id, out);
    }

    let member_names: BTreeSet<&str> = members.iter().map(|id| names[id].as_str()).collect();
    let mut problems = Vec::new();
    for id in &members {
        let name = names[id].as_str();
        if EXEMPT.contains(&name) {
            continue;
        }
        // Direct-only rule.
        if let Some((_, only)) = DIRECT_ONLY.iter().find(|(n, _)| *n == name) {
            for dep in edges.get(id).into_iter().flatten() {
                let dep_name = names[dep].as_str();
                if !only.contains(&dep_name) {
                    problems.push(format!("{name} depends directly on {dep_name}"));
                }
            }
        }
        // Transitive allow-list.
        let mut seen = BTreeSet::new();
        let mut stack = vec![id.clone()];
        while let Some(next) = stack.pop() {
            for dep in edges.get(&next).into_iter().flatten() {
                if seen.insert(dep.clone()) {
                    stack.push(dep.clone());
                }
            }
        }
        for dep in &seen {
            let dep_name = names[dep].as_str();
            if !ALLOWED.contains(&dep_name) && !member_names.contains(dep_name) {
                problems.push(format!("{name} reaches {dep_name}, which is not allowed"));
            }
        }
        // Source scan.
        if let Some(Some(dir)) = dirs.get(id) {
            scan(&dir.join("src"), name, &mut problems)?;
        }
    }

    if problems.is_empty() {
        let checked = member_names.iter().filter(|n| !EXEMPT.contains(n)).count();
        println!("deps: {checked} library crates pass the no-I/O allow-list");
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn scan(dir: &std::path::Path, krate: &str, problems: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            scan(&path, krate, problems)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or_default();
                for bad in FORBIDDEN_PATHS {
                    if code.contains(bad) {
                        problems.push(format!("{krate}: {}:{} names {bad}", path.display(), n + 1));
                    }
                }
            }
        }
    }
    Ok(())
}
