//! `gk3 sheep`: inspect and run the game's compiled scripts.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::rc::Rc;

use gk3_barn::Library;
use gk3_sheep::op::{Op, decode_all};
use std::collections::HashMap;

use gk3_game::api::GameApi;
use gk3_game::state::GameState;
use gk3_sheep::{Script, ThreadState, Type, Vm};

pub fn run(lib: &Library, args: &[String]) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        Some("check") => check(lib),
        Some("disasm") => disasm(lib, args.get(1).ok_or("sheep disasm needs a script name")?),
        Some("apis") => apis(lib),
        Some("run") => run_all(lib),
        Some("call") => call(lib, &args[1..]),
        _ => Err(
            "usage: gk3 sheep check | disasm NAME | apis | run | call SCRIPT FUNCTION [--time T]"
                .into(),
        ),
    }
}

/// Every compiled script, by upper-case name without extension.
pub fn load_all(lib: &Library) -> (BTreeMap<String, Rc<Script>>, Vec<String>) {
    let mut scripts = BTreeMap::new();
    let mut failures = Vec::new();
    for (barn, entry) in lib.assets() {
        let upper = entry.name.to_ascii_uppercase();
        let Some(stem) = upper.strip_suffix(".SHP") else {
            continue;
        };
        let bytes = match barn.read(entry) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{}: {e}", entry.name));
                continue;
            }
        };
        // GK3.SHP is Sheep source, not bytecode: the script the original
        // team built the archives with.
        if !bytes.starts_with(b"GK3Sheep") {
            continue;
        }
        match Script::parse(&bytes).map_err(|e| e.to_string()) {
            Ok(script) => {
                scripts.insert(stem.to_string(), Rc::new(script));
            }
            Err(e) => failures.push(format!("{}: {e}", entry.name)),
        }
    }
    (scripts, failures)
}

fn check(lib: &Library) -> Result<ExitCode, String> {
    let (scripts, mut failures) = load_all(lib);
    let mut functions = 0;
    let mut instructions = 0;
    let mut calls: BTreeMap<String, usize> = BTreeMap::new();
    for (name, script) in &scripts {
        match decode_all(&script.code) {
            Ok(decoded) => {
                instructions += decoded.len();
                for i in &decoded {
                    if matches!(
                        i.op,
                        Op::CallSysFunctionV
                            | Op::CallSysFunctionI
                            | Op::CallSysFunctionF
                            | Op::CallSysFunctionS
                    ) {
                        match script.imports.get(i.operand as usize) {
                            Some(import) => *calls.entry(import.name.clone()).or_default() += 1,
                            None => failures.push(format!(
                                "{name}: call to missing import {} at {}",
                                i.operand, i.at
                            )),
                        }
                    }
                    if matches!(i.op, Op::Branch | Op::BranchGoto | Op::BranchIfZero)
                        && i.operand as usize > script.code.len()
                    {
                        failures.push(format!("{name}: branch past the end at {}", i.at));
                    }
                    if i.op == Op::PushS && script.string(i.operand).is_none() {
                        failures.push(format!(
                            "{name}: no string at {} (used at {})",
                            i.operand, i.at
                        ));
                    }
                }
            }
            Err(e) => failures.push(format!("{name}: {e}")),
        }
        for (function, offset) in &script.functions {
            functions += 1;
            if *offset >= script.code.len() {
                failures.push(format!("{name}: {function} starts past the end"));
            }
        }
    }
    println!("Scripts:       {}", scripts.len());
    println!("Functions:     {functions}");
    println!("Instructions:  {instructions} (every byte of every code block decoded)");
    println!(
        "API functions: {} distinct, {} calls",
        calls.len(),
        calls.values().sum::<usize>()
    );
    println!("Failures:      {}", failures.len());
    crate::report_failures(&failures)
}

fn apis(lib: &Library) -> Result<ExitCode, String> {
    let (scripts, _) = load_all(lib);
    let mut table: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for script in scripts.values() {
        let decoded = decode_all(&script.code).unwrap_or_default();
        for i in decoded.iter().filter(|i| {
            matches!(
                i.op,
                Op::CallSysFunctionV
                    | Op::CallSysFunctionI
                    | Op::CallSysFunctionF
                    | Op::CallSysFunctionS
            )
        }) {
            if let Some(import) = script.imports.get(i.operand as usize) {
                let entry = table
                    .entry(import.name.clone())
                    .or_insert_with(|| (0, signature(import.returns, &import.arguments)));
                entry.0 += 1;
            }
        }
    }
    let mut rows: Vec<_> = table.into_iter().collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
    for (name, (count, sig)) in rows {
        println!("{count:6}  {name}{sig}");
    }
    Ok(ExitCode::SUCCESS)
}

fn signature(returns: Type, args: &[Type]) -> String {
    let t = |t: &Type| match t {
        Type::Void => "void",
        Type::Int => "int",
        Type::Float => "float",
        Type::String => "string",
    };
    let args: Vec<_> = args.iter().map(t).collect();
    format!("({}) -> {}", args.join(", "), t(&returns))
}

fn disasm(lib: &Library, name: &str) -> Result<ExitCode, String> {
    let (scripts, _) = load_all(lib);
    let key = name
        .to_ascii_uppercase()
        .trim_end_matches(".SHP")
        .to_string();
    let script = scripts
        .get(&key)
        .ok_or_else(|| format!("no script {name}"))?;
    let decoded = decode_all(&script.code)?;
    let starts: BTreeMap<usize, &str> = script
        .functions
        .iter()
        .map(|(n, o)| (*o, n.as_str()))
        .collect();
    for i in decoded {
        if let Some(function) = starts.get(&i.at) {
            println!("\n{function}");
        }
        let note = match i.op {
            Op::CallSysFunctionV
            | Op::CallSysFunctionI
            | Op::CallSysFunctionF
            | Op::CallSysFunctionS => script
                .imports
                .get(i.operand as usize)
                .map(|im| format!("// {}", im.name))
                .unwrap_or_default(),
            Op::PushS => script
                .string(i.operand)
                .map(|s| format!("// {s:?}"))
                .unwrap_or_default(),
            Op::PushF => format!("// {}", f32::from_bits(i.operand as u32)),
            Op::LoadI | Op::LoadF | Op::LoadS | Op::StoreI | Op::StoreF | Op::StoreS => script
                .variables
                .get(i.operand as usize)
                .map(|v| format!("// {}", v.name))
                .unwrap_or_default(),
            _ => String::new(),
        };
        if i.op.has_operand() {
            println!(
                "{:8}  {:<18} {:<10} {note}",
                i.at,
                i.op.to_string(),
                i.operand
            );
        } else {
            println!("{:8}  {}", i.at, i.op);
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Runs one thread to the end, finishing every wait at once: what a run
/// looks like when every animation, walk and line of dialogue is instant.
fn run_to_end(
    vm: &mut Vm,
    api: &mut GameApi,
    script: Rc<Script>,
    name: &str,
    function: &str,
) -> Result<usize, String> {
    let id = vm.start(api, script, name, function)?;
    for _ in 0..10_000 {
        let Some(waiting) = api.pending.pop() else {
            break;
        };
        vm.release(api, waiting);
    }
    Ok(id)
}

/// Every function of every script, each from a fresh story state, so one
/// function's leftovers cannot send another down a path the game never takes.
fn run_all(lib: &Library) -> Result<ExitCode, String> {
    let (scripts, _) = load_all(lib);
    let repo: HashMap<String, Rc<Script>> = scripts
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut hashes = Vec::new();
    for pass in 0..2 {
        let mut vm = Vm::new();
        let (mut executed, mut finished, mut waiting, mut faulted) = (0, 0, Vec::new(), Vec::new());
        let (mut calls, mut traced) = (0u64, 0usize);
        let mut unimplemented: BTreeMap<String, usize> = BTreeMap::new();
        let mut combined = 0xcbf2_9ce4_8422_2325u64;
        for (name, script) in &scripts {
            for (function, _) in &script.functions {
                executed += 1;
                let mut state = GameState::new("110A", name);
                let mut api = GameApi::new(&mut state, &repo);
                let id = run_to_end(&mut vm, &mut api, script.clone(), name, function)?;
                match vm.state(id) {
                    Some(ThreadState::Finished) => finished += 1,
                    Some(ThreadState::Waiting) => waiting.push(format!("{name}:{function}")),
                    Some(ThreadState::Faulted(e)) => {
                        faulted.push(format!("{name}:{function}: {e}"))
                    }
                    _ => {}
                }
                calls += api.calls;
                traced += api.trace.len();
                for (n, c) in std::mem::take(&mut api.unimplemented) {
                    *unimplemented.entry(n).or_default() += c;
                }
                drop(api);
                combined = (combined ^ state.hash()).wrapping_mul(0x0100_0000_01b3);
                vm.collect();
            }
        }
        hashes.push(combined);
        if pass == 0 {
            println!("Functions run:     {executed}, each from a fresh state");
            println!("Finished:          {finished}");
            println!(
                "Still waiting:     {} (polling for something only the running game changes)",
                waiting.len()
            );
            println!("Faulted:           {}", faulted.len());
            println!("Instructions:      {}", vm.executed);
            println!("API calls:         {calls}");
            println!("Presentation:      {traced} calls recorded for the engine");
            println!(
                "Not implemented:   {} functions{}",
                unimplemented.len(),
                if unimplemented.is_empty() {
                    String::new()
                } else {
                    format!(
                        ": {}",
                        unimplemented
                            .iter()
                            .map(|(n, c)| format!("{n} ({c})"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            );
            for w in &waiting {
                println!("  WAITING {w}");
            }
            for f in &faulted {
                println!("  FAULT {f}");
            }
        }
    }
    println!(
        "State hash:        {:016x}, {:016x} ({})",
        hashes[0],
        hashes[1],
        if hashes[0] == hashes[1] {
            "identical across two runs"
        } else {
            "DIFFERENT"
        }
    );
    Ok(if hashes[0] == hashes[1] {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Runs one function and prints what it did.
fn call(lib: &Library, args: &[String]) -> Result<ExitCode, String> {
    let (Some(script_name), Some(function)) = (args.first(), args.get(1)) else {
        return Err("usage: gk3 sheep call SCRIPT FUNCTION [--time 110A]".into());
    };
    let time = args
        .iter()
        .position(|a| a == "--time")
        .and_then(|i| args.get(i + 1))
        .map_or("110A", |s| s.as_str());
    let (scripts, _) = load_all(lib);
    let repo: HashMap<String, Rc<Script>> = scripts
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let key = script_name.to_ascii_uppercase();
    let script = repo
        .get(&key)
        .cloned()
        .ok_or_else(|| format!("no script {script_name}"))?;
    let function = if function.ends_with('$') {
        function.clone()
    } else {
        format!("{function}$")
    };
    let mut state = GameState::new(time, &key);
    let mut api = GameApi::new(&mut state, &repo);
    let mut vm = Vm::new();
    let id = run_to_end(&mut vm, &mut api, script, &key, &function)?;
    for line in &api.trace {
        println!("{line}");
    }
    if !api.unimplemented.is_empty() {
        println!("not implemented: {:?}", api.unimplemented);
    }
    println!("-> {:?}", vm.state(id));
    Ok(ExitCode::SUCCESS)
}
