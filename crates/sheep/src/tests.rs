//! VM tests on hand-assembled bytecode.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::op::Op;
use crate::script::{Import, Script, Variable};
use crate::vm::{CallContext, Host, Outcome, ThreadState, Vm};
use crate::{Type, Value};

/// Assembles (op, operand) pairs.
fn code(instructions: &[(Op, i32)]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(op, operand) in instructions {
        out.push(op as u8);
        if op.has_operand() {
            out.extend(operand.to_le_bytes());
        }
    }
    out
}

fn import(name: &str, returns: Type, arguments: &[Type]) -> Import {
    Import {
        name: name.into(),
        returns,
        arguments: arguments.to_vec(),
    }
}

#[derive(Default)]
struct Recorder {
    calls: Vec<String>,
    /// Names of functions that report Pending, and the threads waiting.
    slow: Vec<&'static str>,
    pending: Vec<usize>,
    scripts: HashMap<String, Rc<Script>>,
}

impl Host for Recorder {
    fn call(&mut self, name: &str, args: &[Value], _: Type, context: &CallContext) -> Outcome {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        self.calls.push(format!("{name}({})", args.join(", ")));
        if name == "CallSheep" {
            return Outcome::Spawn {
                script: "OTHER".into(),
                function: "Go$".into(),
            };
        }
        if self.slow.contains(&name) {
            self.pending.push(context.thread);
            return Outcome::Pending(Value::Int(0));
        }
        Outcome::Done(Value::Int(41))
    }

    fn script(&mut self, name: &str) -> Option<Rc<Script>> {
        self.scripts.get(name).cloned()
    }
}

fn script(imports: Vec<Import>, strings: &[(i32, &str)], instructions: &[(Op, i32)]) -> Rc<Script> {
    Rc::new(Script {
        imports,
        strings: strings
            .iter()
            .map(|&(o, s)| (o, s.to_string()))
            .collect::<BTreeMap<_, _>>(),
        variables: vec![Variable {
            name: "x".into(),
            initial: Value::Int(0),
        }],
        functions: vec![("Main$".into(), 0)],
        code: code(instructions),
    })
}

#[test]
fn calls_strings_branches_and_variables() {
    use Op::*;
    // x = Answer("hi") + 1; if (x == 42) Log("yes")
    let s = script(
        vec![
            import("Answer", Type::Int, &[Type::String]),
            import("Log", Type::Void, &[Type::String]),
        ],
        &[(0, "hi"), (3, "yes")],
        &[
            (PushS, 0),
            (GetString, 0),
            (PushI, 1),
            (CallSysFunctionI, 0),
            (PushI, 1),
            (AddI, 0),
            (StoreI, 0),
            (LoadI, 0),
            (PushI, 42),
            (IsEqualI, 0),
            (BranchIfZero, 60),
            (PushS, 3),
            (GetString, 0),
            (PushI, 1),
            (CallSysFunctionV, 1),
            (Pop, 0),
            (ReturnV, 0),
        ],
    );
    let mut host = Recorder::default();
    let mut vm = Vm::new();
    let id = vm.start(&mut host, s, "T", "main$").unwrap();
    assert_eq!(vm.state(id), Some(&ThreadState::Finished));
    assert_eq!(host.calls, ["Answer(\"hi\")", "Log(\"yes\")"]);
}

#[test]
fn wait_blocks_suspend_until_released() {
    use Op::*;
    let s = script(
        vec![
            import("Walk", Type::Void, &[]),
            import("After", Type::Void, &[]),
        ],
        &[],
        &[
            (BeginWait, 0),
            (PushI, 0),
            (CallSysFunctionV, 0),
            (Pop, 0),
            (EndWait, 0),
            (PushI, 0),
            (CallSysFunctionV, 1),
            (Pop, 0),
            (ReturnV, 0),
        ],
    );
    let mut host = Recorder {
        slow: vec!["Walk"],
        ..Default::default()
    };
    let mut vm = Vm::new();
    let id = vm.start(&mut host, s, "T", "Main$").unwrap();
    assert_eq!(vm.state(id), Some(&ThreadState::Waiting));
    assert_eq!(host.calls, ["Walk()"]);
    let waiting = host.pending[0];
    vm.release(&mut host, waiting);
    assert_eq!(vm.state(id), Some(&ThreadState::Finished));
    assert_eq!(host.calls, ["Walk()", "After()"]);
}

#[test]
fn waiting_on_another_script() {
    use Op::*;
    let other = script(
        vec![import("Walk", Type::Void, &[])],
        &[],
        &[
            (BeginWait, 0),
            (PushI, 0),
            (CallSysFunctionV, 0),
            (Pop, 0),
            (EndWait, 0),
            (ReturnV, 0),
        ],
    );
    let other = Rc::new(Script {
        functions: vec![("Go$".into(), 0)],
        ..(*other).clone()
    });
    let main = script(
        vec![
            import("CallSheep", Type::Void, &[]),
            import("After", Type::Void, &[]),
        ],
        &[],
        &[
            (BeginWait, 0),
            (PushI, 0),
            (CallSysFunctionV, 0),
            (Pop, 0),
            (EndWait, 0),
            (PushI, 0),
            (CallSysFunctionV, 1),
            (Pop, 0),
            (ReturnV, 0),
        ],
    );
    let mut host = Recorder {
        slow: vec!["Walk"],
        ..Default::default()
    };
    host.scripts.insert("OTHER".into(), other);
    let mut vm = Vm::new();
    let id = vm.start(&mut host, main, "T", "Main$").unwrap();
    assert_eq!(
        vm.state(id),
        Some(&ThreadState::Waiting),
        "waits on the callee"
    );
    // The callee's walk ends; the callee finishes and releases the caller.
    let waiting = host.pending[0];
    vm.release(&mut host, waiting);
    assert_eq!(vm.state(id), Some(&ThreadState::Finished));
    assert_eq!(host.calls.last().unwrap(), "After()");
}

#[test]
fn conversions_reach_down_the_stack() {
    use Op::*;
    // 1 + 0.5 as the compiler emits it: the int is one below the top.
    let s = script(
        vec![import("Show", Type::Void, &[Type::Float])],
        &[],
        &[
            (PushI, 1),
            (PushF, 0.5f32.to_bits() as i32),
            (IToF, 1),
            (AddF, 0),
            (PushI, 1),
            (CallSysFunctionV, 0),
            (Pop, 0),
            (ReturnV, 0),
        ],
    );
    let mut host = Recorder::default();
    Vm::new().start(&mut host, s, "T", "Main$").unwrap();
    assert_eq!(host.calls, ["Show(1.5)"]);
}

#[test]
fn runaway_loops_fault() {
    use Op::*;
    let s = script(vec![], &[], &[(Branch, 0)]);
    let mut vm = Vm::new();
    vm.budget = 1000;
    let id = vm.start(&mut Recorder::default(), s, "T", "Main$").unwrap();
    assert!(matches!(vm.state(id), Some(ThreadState::Faulted(_))));
}

#[test]
fn bad_bytecode_faults_rather_than_panicking() {
    let s = Rc::new(Script {
        functions: vec![("Main$".into(), 0)],
        code: vec![0x0C, 0x16, 0x16],
        ..Default::default()
    });
    let mut vm = Vm::new();
    let id = vm.start(&mut Recorder::default(), s, "T", "Main$").unwrap();
    assert!(matches!(vm.state(id), Some(ThreadState::Faulted(_))));
}

#[test]
fn running_threads_of_a_script_share_its_variables() {
    use Op::*;
    // x = x + 1; wait Walk(); Report(x)
    let s = script(
        vec![
            import("Walk", Type::Void, &[]),
            import("Report", Type::Void, &[Type::Int]),
        ],
        &[],
        &[
            (LoadI, 0),
            (PushI, 1),
            (AddI, 0),
            (StoreI, 0),
            (BeginWait, 0),
            (PushI, 0),
            (CallSysFunctionV, 0),
            (Pop, 0),
            (EndWait, 0),
            (LoadI, 0),
            (PushI, 1),
            (CallSysFunctionV, 1),
            (Pop, 0),
            (ReturnV, 0),
        ],
    );
    let mut host = Recorder {
        slow: vec!["Walk"],
        ..Default::default()
    };
    let mut vm = Vm::new();
    vm.start(&mut host, Rc::clone(&s), "T", "Main$").unwrap();
    vm.start(&mut host, Rc::clone(&s), "t", "Main$").unwrap();
    for waiting in std::mem::take(&mut host.pending) {
        vm.release(&mut host, waiting);
    }
    assert_eq!(host.calls[2..], ["Report(2)", "Report(2)"]);
    // With none running, a new thread starts from the initial values.
    vm.collect();
    vm.start(&mut host, s, "T", "Main$").unwrap();
    let waiting = host.pending.pop().unwrap();
    vm.release(&mut host, waiting);
    assert_eq!(host.calls.last().unwrap(), "Report(1)");
}
