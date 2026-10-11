//! `gk3 actions`: check and query the action files.

use std::collections::BTreeMap;
use std::process::ExitCode;

use gk3_barn::Library;
use gk3_game::actions::ActionSet;
use gk3_sheep::compile::compile_action;

pub fn run(lib: &Library, args: &[String]) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        Some("check") => check(lib),
        _ => Err("usage: gk3 actions check".into()),
    }
}

fn check(lib: &Library) -> Result<ExitCode, String> {
    let mut files = 0;
    let mut set = ActionSet::default();
    for (barn, entry) in lib.assets() {
        if !entry.name.to_ascii_uppercase().ends_with(".NVC") {
            continue;
        }
        let bytes = barn.read(entry).map_err(|e| e.to_string())?;
        set.add_file(&bytes, &entry.name);
        files += 1;
    }
    let mut failures = Vec::new();
    let mut functions: BTreeMap<String, usize> = BTreeMap::new();
    let mut statements = 0;
    for rule in &set.rules {
        match compile_action(&rule.script) {
            Ok(script) => {
                statements += gk3_sheep::op::decode_all(&script.code)
                    .map(|ops| {
                        ops.iter()
                            .filter(|i| i.op == gk3_sheep::op::Op::CallSysFunctionV)
                            .count()
                    })
                    .unwrap_or(0);
                for import in script.imports {
                    *functions.entry(import.name).or_default() += 1;
                }
            }
            Err(e) => failures.push(format!(
                "{} {}:{} in {}: {e}",
                rule.noun, rule.verb, rule.case, rule.source
            )),
        }
    }
    let nouns: std::collections::BTreeSet<_> = set.rules.iter().map(|r| &r.noun).collect();
    let verbs: std::collections::BTreeSet<_> = set.rules.iter().map(|r| &r.verb).collect();
    println!("Action files: {files}");
    println!(
        "Rules:        {} ({} nouns, {} verbs)",
        set.rules.len(),
        nouns.len(),
        verbs.len()
    );
    println!(
        "Compiled:     {} rules, {statements} calls to {} distinct functions",
        set.rules.len() - failures.len(),
        functions.len()
    );
    println!("Failures:     {}", failures.len());
    crate::report_failures(&failures)
}
