//! Textual navigation index only; production declarations own the lock policy.
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use vanllia_verification::*;
fn source_files(path: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_dir() {
            source_files(&path, files)?;
        } else if path.extension().is_some_and(|s| s == "rs") {
            files.push(path);
        }
    }
    Ok(())
}
pub fn run(root: &Path) -> Result<()> {
    let order = fs::read_to_string(root.join("server/src/metal/riscv/sync/order/mod.rs"))?;
    let classes: Vec<_> = Regex::new(r"(?m)^    (\w+),$")?
        .captures_iter(
            order
                .split("pub enum Class {")
                .nth(1)
                .ok_or("missing lock Class")?
                .split("impl Class {")
                .next()
                .unwrap(),
        )
        .map(|c| c[1].to_owned())
        .collect();
    let mut reach = BTreeMap::<String, BTreeSet<String>>::new();
    for c in Regex::new(r"(?m)^    (\w+) => \[([^\n]*)\],")?.captures_iter(&order) {
        reach.insert(
            c[1].into(),
            Regex::new(r"\w+")?
                .find_iter(&c[2])
                .map(|m| m.as_str().to_owned())
                .collect(),
        );
    }
    let empty = BTreeSet::new();
    let reduced: BTreeMap<_, _> = classes
        .iter()
        .map(|a| {
            let targets = reach.get(a).unwrap_or(&empty);
            let values: Vec<_> = targets
                .iter()
                .filter(|b| {
                    !targets
                        .iter()
                        .any(|c| c != *b && reach.get(c).unwrap_or(&empty).contains(*b))
                })
                .cloned()
                .collect();
            (a.clone(), values)
        })
        .collect();
    let mut fields: BTreeMap<_, Vec<Value>> = classes.iter().map(|a| (a.clone(), vec![])).collect();
    let mut calls = vec![];
    let mut files = vec![];
    source_files(&root.join("server/src"), &mut files)?;
    files.sort();
    let declaration =
        Regex::new(r"(?:LegacyLock|Lock)\s*<\s*(?:crate::metal::sync::)?order::(\w+)")?;
    let functions = Regex::new(r"\bfn\s+(\w+)")?;
    let operations = Regex::new(
        r"\.(lock|try_lock|with|pair|try_pair|reenter|legacy_lock|legacy_try_lock|try_source_borrow)\s*\(",
    )?;
    for path in files {
        let source = fs::read_to_string(&path)?;
        let relative = path.strip_prefix(root)?.display().to_string();
        let line_at = |at: usize| source[..at].bytes().filter(|b| *b == b'\n').count() + 1;
        let text_at = |line: usize| source.lines().nth(line - 1).unwrap_or("").trim().to_owned();
        for c in declaration.captures_iter(&source) {
            if let Some(field) = fields.get_mut(&c[1]) {
                let line = line_at(c.get(0).unwrap().start());
                field.push(json!({"path":relative,"line":line,"declaration":text_at(line)}));
            }
        }
        let functions: Vec<_> = functions
            .captures_iter(&source)
            .map(|c| (c.get(0).unwrap().start(), c[1].to_owned()))
            .collect();
        for c in operations.captures_iter(&source) {
            let at = c.get(0).unwrap().start();
            let line = line_at(at);
            let owner = functions
                .iter()
                .rev()
                .find(|(start, _)| *start < at)
                .map(|(_, name)| name.as_str())
                .unwrap_or("<static>");
            calls.push(json!({"path":relative,"line":line,"enclosing_function":owner,"operation":&c[1],"source":text_at(line)}));
        }
    }
    let report = json!({"scope":"textual current declarations and call sites; see README.md for ownership and lifetime evidence","classes":classes.iter().map(|a|json!({"class":a,"typed_declarations":fields[a],"minimal_declared_predecessors":classes.iter().filter(|b|reduced[*b].contains(a)).collect::<Vec<_>>(),"minimal_declared_successors":reduced[a]})).collect::<Vec<_>>(),"acquisition_sites":calls});
    let catalogue = root.join("docs/synchronization/acquisitions.json");
    let previous: Option<Value> = fs::read(&catalogue)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if previous.as_ref() != Some(&report) {
        json(&catalogue, &report)?;
    }
    let mut text = String::from(
        "# Current lock fields and acquisition relations\n\nGenerated with `cargo +nightly run --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu -- inventory --kernel .`. The [audit](README.md) supplies owner, lifetime, failure, IRQ and migration obligations. The [complete call-site catalogue](acquisitions.json) links each textual acquisition to its source function; overloaded names and `with` calls require the source context. This is a review index, not execution coverage.\n\nOnly this table’s predecessors/successors are a display of the transitive reduction of the production `relations!` closure. They are permitted directions, not claims that every possible edge is executed. All omitted ordinary combinations are forbidden; controlled peers and source-borrow try admission are separate protocols.\n\n| Class | Current typed storage/navigation | Minimal declared predecessors | Minimal declared successors |\n| --- | --- | --- | --- |\n",
    );
    for a in &classes {
        let mut refs = fields[a]
            .iter()
            .map(|x| {
                let path = x["path"].as_str().unwrap();
                let line = x["line"].as_u64().unwrap();
                format!(
                    "[{}:{line}](../../{path}#L{line})",
                    path.strip_prefix("server/src/").unwrap_or(path)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        if refs.is_empty() {
            refs = if a == "ProbeRegistry" {
                "injected guest probe only"
            } else {
                "see constructor migration table and owner audit"
            }
            .into();
        }
        let predecessors = classes
            .iter()
            .filter(|b| reduced[*b].contains(a))
            .map(|b| format!("`{b}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let successors = reduced[a]
            .iter()
            .map(|b| format!("`{b}`"))
            .collect::<Vec<_>>()
            .join(", ");
        text += &format!("| `{a}` | {refs} | {predecessors} | {successors} |\n");
    }
    fs::write(root.join("docs/synchronization/fields.md"), text)?;
    println!(
        "{} classes; {} textual call sites",
        classes.len(),
        calls.len()
    );
    Ok(())
}
