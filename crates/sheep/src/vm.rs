//! The Sheep virtual machine: a stack machine with resumable threads.
//!
//! Matches the original's conventions. A call pushes its arguments, then
//! their count; every system call pushes a result, void ones included, and
//! the compiler pops it. A `wait { ... }` block suspends at `EndWait` while
//! any waitable call made inside it is still running; the host releases the
//! thread when they finish.

use std::cell::RefCell;
use std::rc::Rc;

use crate::op::{Op, decode};
use crate::script::Script;
use crate::{Type, Value};

pub type ThreadId = usize;

/// What a system call did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Finished, with this result.
    Done(Value),
    /// Started something that takes time. The host will call
    /// [`Vm::release`] for this thread when it ends. Outside a wait block
    /// the script does not wait for it.
    Pending(Value),
    /// Run a Sheep function, in another script or (with `script` empty) the
    /// caller's own. Inside a wait block the caller waits for it.
    Spawn { script: String, function: String },
}

/// Information about a call, for hosts that care which thread made it.
pub struct CallContext<'a> {
    pub thread: ThreadId,
    pub script: &'a str,
    pub in_wait: bool,
}

/// The game, as seen from a script.
pub trait Host {
    fn call(&mut self, name: &str, args: &[Value], returns: Type, context: &CallContext)
    -> Outcome;
    /// A script by name, for calls between scripts.
    fn script(&mut self, name: &str) -> Option<Rc<Script>>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThreadState {
    Running,
    /// Suspended at the end of a wait block.
    Waiting,
    Finished,
    Faulted(String),
}

struct Thread {
    script: Rc<Script>,
    script_name: String,
    function: String,
    /// The script's variables, shared with every other running thread of
    /// the same script, as the original does: the temple fight's threads
    /// pass Gabriel's and the demon's positions through them. They start
    /// from their initial values again once no thread of it is running.
    vars: Rc<RefCell<Vec<Value>>>,
    pc: usize,
    stack: Vec<Value>,
    in_wait: bool,
    waiting_on: u32,
    state: ThreadState,
    /// A thread that waits on this one finishing.
    parent: Option<ThreadId>,
}

/// How many instructions a single run may execute before it is stopped.
/// Scripts are data, and some poll in loops that only end when the game
/// changes underneath them.
pub const DEFAULT_BUDGET: usize = 200_000;

pub struct Vm {
    threads: Vec<Option<Thread>>,
    pub budget: usize,
    /// Calls between scripts nest; the data calls in circles.
    depth: usize,
    /// Instructions executed in total, for reports.
    pub executed: u64,
}

impl Default for Vm {
    fn default() -> Self {
        Self {
            threads: Vec::new(),
            budget: DEFAULT_BUDGET,
            depth: 0,
            executed: 0,
        }
    }
}

impl Vm {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts `function` (with its `$`) of a script and runs it until it
    /// finishes, faults or waits.
    pub fn start(
        &mut self,
        host: &mut dyn Host,
        script: Rc<Script>,
        script_name: &str,
        function: &str,
    ) -> Result<ThreadId, String> {
        self.spawn(host, script, script_name, function, None)
    }

    fn spawn(
        &mut self,
        host: &mut dyn Host,
        script: Rc<Script>,
        script_name: &str,
        function: &str,
        parent: Option<ThreadId>,
    ) -> Result<ThreadId, String> {
        let pc = script
            .function(function)
            .ok_or_else(|| format!("{script_name} has no function {function}"))?;
        let running = self.threads.iter().flatten().find(|t| {
            matches!(t.state, ThreadState::Running | ThreadState::Waiting)
                && t.script_name.eq_ignore_ascii_case(script_name)
        });
        let vars = match running {
            Some(t) => Rc::clone(&t.vars),
            None => Rc::new(RefCell::new(
                script.variables.iter().map(|v| v.initial.clone()).collect(),
            )),
        };
        let thread = Thread {
            script,
            script_name: script_name.to_string(),
            function: function.to_string(),
            vars,
            pc,
            stack: Vec::new(),
            in_wait: false,
            waiting_on: 0,
            state: ThreadState::Running,
            parent,
        };
        let id = match self.threads.iter().position(Option::is_none) {
            Some(free) => {
                self.threads[free] = Some(thread);
                free
            }
            None => {
                self.threads.push(Some(thread));
                self.threads.len() - 1
            }
        };
        if self.depth > 32 {
            self.fault(host, id, "scripts call each other too deeply".into());
        } else {
            self.depth += 1;
            self.run(host, id);
            self.depth -= 1;
        }
        Ok(id)
    }

    pub fn state(&self, id: ThreadId) -> Option<&ThreadState> {
        self.threads.get(id)?.as_ref().map(|t| &t.state)
    }

    /// Which function a thread runs, for reports.
    pub fn describe(&self, id: ThreadId) -> Option<String> {
        let t = self.threads.get(id)?.as_ref()?;
        Some(format!("{}:{}", t.script_name, t.function))
    }

    /// Threads that have not finished.
    pub fn waiting(&self) -> Vec<ThreadId> {
        self.threads
            .iter()
            .enumerate()
            .filter_map(|(i, t)| (t.as_ref()?.state == ThreadState::Waiting).then_some(i))
            .collect()
    }

    /// Forgets finished and faulted threads.
    pub fn collect(&mut self) {
        for slot in &mut self.threads {
            if slot
                .as_ref()
                .is_some_and(|t| matches!(t.state, ThreadState::Finished | ThreadState::Faulted(_)))
            {
                *slot = None;
            }
        }
    }

    /// One thing a thread was waiting for has finished. When nothing is left,
    /// it carries on from after its wait block.
    pub fn release(&mut self, host: &mut dyn Host, id: ThreadId) {
        let Some(Some(t)) = self.threads.get_mut(id) else {
            return;
        };
        t.waiting_on = t.waiting_on.saturating_sub(1);
        if t.waiting_on == 0 && t.state == ThreadState::Waiting {
            t.state = ThreadState::Running;
            t.in_wait = false;
            self.run(host, id);
        }
    }

    fn fault(&mut self, host: &mut dyn Host, id: ThreadId, message: String) {
        if let Some(Some(t)) = self.threads.get_mut(id) {
            t.state = ThreadState::Faulted(message);
        }
        self.finish(host, id);
    }

    /// Releases whoever waits on a thread that has stopped.
    fn finish(&mut self, host: &mut dyn Host, id: ThreadId) {
        let parent = self.threads[id].as_mut().and_then(|t| t.parent.take());
        if let Some(parent) = parent {
            self.release(host, parent);
        }
    }

    fn run(&mut self, host: &mut dyn Host, id: ThreadId) {
        let mut budget = self.budget;
        loop {
            let Some(Some(t)) = self.threads.get_mut(id) else {
                return;
            };
            if t.state != ThreadState::Running {
                return;
            }
            if budget == 0 {
                let message = format!("ran {} instructions without finishing", self.budget);
                return self.fault(host, id, message);
            }
            budget -= 1;
            self.executed += 1;

            let instruction = match decode(&t.script.code, t.pc) {
                Ok(i) => i,
                // Running off the end is how a function without a return
                // finishes.
                Err(_) if t.pc >= t.script.code.len() => {
                    t.state = ThreadState::Finished;
                    return self.finish(host, id);
                }
                Err(e) => return self.fault(host, id, e),
            };
            t.pc += instruction.len();
            let operand = instruction.operand;

            macro_rules! pop {
                () => {
                    match t.stack.pop() {
                        Some(v) => v,
                        None => {
                            return self.fault(
                                host,
                                id,
                                format!("stack empty at {}", instruction.at),
                            )
                        }
                    }
                };
            }
            macro_rules! binary {
                ($f:expr) => {{
                    let b = pop!();
                    let a = pop!();
                    #[allow(clippy::redundant_closure_call)]
                    t.stack.push($f(a, b));
                }};
            }
            let int = |b: bool| Value::Int(b as i32);

            match instruction.op {
                Op::SitnSpin | Op::DebugBreakpoint => {}
                Op::Yield => return,
                Op::CallSysFunctionV
                | Op::CallSysFunctionI
                | Op::CallSysFunctionF
                | Op::CallSysFunctionS => {
                    let Some(import) = t.script.imports.get(operand as usize).cloned() else {
                        return self.fault(host, id, format!("no import {operand}"));
                    };
                    let count = pop!().int().max(0) as usize;
                    if t.stack.len() < count {
                        return self.fault(
                            host,
                            id,
                            format!("{} wants {count} arguments", import.name),
                        );
                    }
                    let raw = t.stack.split_off(t.stack.len() - count);
                    let args: Vec<Value> = raw
                        .iter()
                        .enumerate()
                        .map(|(i, v)| match import.arguments.get(i) {
                            Some(&ty) => v.to_type(ty),
                            None => v.clone(),
                        })
                        .collect();
                    let in_wait = t.in_wait;
                    let script_name = t.script_name.clone();
                    let context = CallContext {
                        thread: id,
                        script: &script_name,
                        in_wait,
                    };
                    let outcome = host.call(&import.name, &args, import.returns, &context);
                    let returned = match outcome {
                        Outcome::Done(v) => v,
                        Outcome::Pending(v) => {
                            if in_wait {
                                self.threads[id].as_mut().unwrap().waiting_on += 1;
                            }
                            v
                        }
                        Outcome::Spawn { script, function } => {
                            let (name, target) = if script.is_empty() {
                                let t = self.threads[id].as_ref().unwrap();
                                (t.script_name.clone(), Some(t.script.clone()))
                            } else {
                                (script.clone(), host.script(&script))
                            };
                            if let Some(target) = target {
                                let parent = in_wait.then_some(id);
                                if in_wait {
                                    self.threads[id].as_mut().unwrap().waiting_on += 1;
                                }
                                if self.spawn(host, target, &name, &function, parent).is_err()
                                    && in_wait
                                {
                                    // Nothing to wait for after all.
                                    let t = self.threads[id].as_mut().unwrap();
                                    t.waiting_on = t.waiting_on.saturating_sub(1);
                                }
                            }
                            Value::Int(0)
                        }
                    };
                    let t = self.threads[id].as_mut().unwrap();
                    let value = match instruction.op {
                        Op::CallSysFunctionF => Value::Float(returned.float()),
                        Op::CallSysFunctionS => Value::Str(returned.string()),
                        _ => Value::Int(returned.int()),
                    };
                    t.stack.push(value);
                }
                Op::Branch | Op::BranchGoto => t.pc = operand as usize,
                Op::BranchIfZero => {
                    if pop!().int() == 0 {
                        t.pc = operand as usize;
                    }
                }
                Op::BeginWait => t.in_wait = true,
                Op::EndWait => {
                    if t.waiting_on > 0 {
                        t.state = ThreadState::Waiting;
                        return;
                    }
                    t.in_wait = false;
                }
                Op::ReturnV => {
                    t.state = ThreadState::Finished;
                    return self.finish(host, id);
                }
                Op::StoreI | Op::StoreF | Op::StoreS => {
                    let v = pop!();
                    if let Some(slot) = t.vars.borrow_mut().get_mut(operand as usize) {
                        *slot = match instruction.op {
                            Op::StoreI => Value::Int(v.int()),
                            Op::StoreF => Value::Float(v.float()),
                            _ => Value::Str(v.string()),
                        };
                    }
                }
                Op::LoadI | Op::LoadF | Op::LoadS => {
                    let v = t
                        .vars
                        .borrow()
                        .get(operand as usize)
                        .cloned()
                        .unwrap_or(Value::Int(0));
                    t.stack.push(v);
                }
                Op::PushI => t.stack.push(Value::Int(operand)),
                Op::PushF => t.stack.push(Value::Float(f32::from_bits(operand as u32))),
                // A string is pushed as its pool offset; GetString resolves it.
                Op::PushS => t.stack.push(Value::Int(operand)),
                Op::GetString => {
                    let offset = pop!().int();
                    let text = t.script.string(offset).unwrap_or("").to_string();
                    t.stack.push(Value::Str(text.into()));
                }
                Op::Pop => {
                    pop!();
                }
                Op::AddI => binary!(|a: Value, b: Value| Value::Int(a.int().wrapping_add(b.int()))),
                Op::AddF => binary!(|a: Value, b: Value| Value::Float(a.float() + b.float())),
                Op::SubtractI => {
                    binary!(|a: Value, b: Value| Value::Int(a.int().wrapping_sub(b.int())))
                }
                Op::SubtractF => binary!(|a: Value, b: Value| Value::Float(a.float() - b.float())),
                Op::MultiplyI => {
                    binary!(|a: Value, b: Value| Value::Int(a.int().wrapping_mul(b.int())))
                }
                Op::MultiplyF => binary!(|a: Value, b: Value| Value::Float(a.float() * b.float())),
                // Division by zero gives zero, as in the original.
                Op::DivideI => binary!(|a: Value, b: Value| Value::Int(
                    a.int().checked_div(b.int()).unwrap_or(0)
                )),
                Op::DivideF => binary!(|a: Value, b: Value| {
                    let d = b.float();
                    Value::Float(if d == 0.0 { 0.0 } else { a.float() / d })
                }),
                Op::Modulo => binary!(|a: Value, b: Value| Value::Int(
                    a.int().checked_rem(b.int()).unwrap_or(0)
                )),
                Op::NegateI => {
                    let v = pop!();
                    t.stack.push(Value::Int(v.int().wrapping_neg()));
                }
                Op::NegateF => {
                    let v = pop!();
                    t.stack.push(Value::Float(-v.float()));
                }
                Op::IsEqualI => binary!(|a: Value, b: Value| int(a.int() == b.int())),
                Op::IsEqualF => {
                    binary!(|a: Value, b: Value| int((a.float() - b.float()).abs() < f32::EPSILON))
                }
                Op::IsNotEqualI => binary!(|a: Value, b: Value| int(a.int() != b.int())),
                Op::IsNotEqualF => {
                    binary!(|a: Value, b: Value| int((a.float() - b.float()).abs() >= f32::EPSILON))
                }
                Op::IsGreaterI => binary!(|a: Value, b: Value| int(a.int() > b.int())),
                Op::IsGreaterF => binary!(|a: Value, b: Value| int(a.float() > b.float())),
                Op::IsLessI => binary!(|a: Value, b: Value| int(a.int() < b.int())),
                Op::IsLessF => binary!(|a: Value, b: Value| int(a.float() < b.float())),
                Op::IsGreaterEqualI => binary!(|a: Value, b: Value| int(a.int() >= b.int())),
                Op::IsGreaterEqualF => binary!(|a: Value, b: Value| int(a.float() >= b.float())),
                Op::IsLessEqualI => binary!(|a: Value, b: Value| int(a.int() <= b.int())),
                Op::IsLessEqualF => binary!(|a: Value, b: Value| int(a.float() <= b.float())),
                Op::And => binary!(|a: Value, b: Value| int(a.int() != 0 && b.int() != 0)),
                Op::Or => binary!(|a: Value, b: Value| int(a.int() != 0 || b.int() != 0)),
                Op::Not => {
                    let v = pop!();
                    t.stack.push(int(v.int() == 0));
                }
                // The operand says how far down the stack the value is.
                Op::IToF | Op::FToI => {
                    let depth = operand.max(0) as usize;
                    let Some(index) = t.stack.len().checked_sub(depth + 1) else {
                        return self.fault(
                            host,
                            id,
                            format!("{} reaches below the stack", instruction.op),
                        );
                    };
                    let v = &t.stack[index];
                    t.stack[index] = if instruction.op == Op::IToF {
                        Value::Float(v.int() as f32)
                    } else {
                        Value::Int(v.float() as i32)
                    };
                }
            }
        }
    }
}
