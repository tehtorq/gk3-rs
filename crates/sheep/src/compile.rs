//! Compiling the Sheep written in action files into bytecode.
//!
//! An action's script is a straight run of calls: `wait StartVoiceOver(...);
//! SetLocation("HAL");`, sometimes grouped as `wait { a(); b(); }`. There are
//! no branches or variables; those live in case conditions and in compiled
//! scripts. Compiling these to the machine's own bytecode, rather than
//! interpreting them separately, means an action waits on animations,
//! dialogue and other scripts exactly as a compiled script does.
//!
//! Arguments are literals. Anything else is refused, whole: half an action is
//! worse than none, because the half that ran has already changed the story.

use std::collections::BTreeMap;

use crate::expr::Expr;
use crate::op::Op;
use crate::script::{Import, Script};
use crate::{Type, Value};

/// The name of the single function a compiled action holds.
pub const ENTRY: &str = "Action$";

struct Builder {
    code: Vec<u8>,
    imports: Vec<Import>,
    strings: BTreeMap<i32, String>,
    pool_size: i32,
}

impl Builder {
    fn op(&mut self, op: Op, operand: i32) {
        self.code.push(op as u8);
        if op.has_operand() {
            self.code.extend(operand.to_le_bytes());
        }
    }

    fn string(&mut self, s: &str) -> i32 {
        if let Some((&offset, _)) = self.strings.iter().find(|(_, v)| v.as_str() == s) {
            return offset;
        }
        let offset = self.pool_size;
        self.pool_size += s.len() as i32 + 1;
        self.strings.insert(offset, s.to_string());
        offset
    }

    fn import(&mut self, name: &str, args: &[Value]) -> i32 {
        if let Some(i) = self
            .imports
            .iter()
            .position(|i| i.name.eq_ignore_ascii_case(name))
        {
            return i as i32;
        }
        self.imports.push(Import {
            name: name.to_string(),
            returns: Type::Void,
            arguments: args
                .iter()
                .map(|a| match a {
                    Value::Int(_) => Type::Int,
                    Value::Float(_) => Type::Float,
                    Value::Str(_) => Type::String,
                })
                .collect(),
        });
        self.imports.len() as i32 - 1
    }

    fn call(&mut self, name: &str, args: &[Value]) {
        for arg in args {
            match arg {
                Value::Int(i) => self.op(Op::PushI, *i),
                Value::Float(f) => self.op(Op::PushF, f.to_bits() as i32),
                Value::Str(s) => {
                    let offset = self.string(s);
                    self.op(Op::PushS, offset);
                    self.op(Op::GetString, 0);
                }
            }
        }
        self.op(Op::PushI, args.len() as i32);
        let import = self.import(name, args);
        self.op(Op::CallSysFunctionV, import);
        self.op(Op::Pop, 0);
    }
}

/// One call, as written.
fn parse_call(text: &str) -> Result<(String, Vec<Value>), String> {
    match Expr::parse(text)? {
        Expr::Call(name, args) => {
            let args = args
                .into_iter()
                .map(|a| match a {
                    Expr::Literal(v) => Ok(v),
                    Expr::Unary('-', inner) => match *inner {
                        Expr::Literal(Value::Int(i)) => Ok(Value::Int(-i)),
                        Expr::Literal(Value::Float(f)) => Ok(Value::Float(-f)),
                        other => Err(format!("argument {other:?} is not a literal")),
                    },
                    other => Err(format!("argument {other:?} is not a literal")),
                })
                .collect::<Result<_, _>>()?;
            Ok((name, args))
        }
        other => Err(format!("{other:?} is not a call")),
    }
}

/// Splits on `;` outside quotes and braces.
fn statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, false, 0);
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '{' if !quoted => depth += 1,
            '}' if !quoted => {
                depth -= 1;
                // A closing brace ends a group statement too.
                if depth == 0 {
                    out.push(text[start..=i].to_string());
                    start = i + 1;
                }
            }
            ';' if !quoted && depth == 0 => {
                out.push(text[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(text[start..].to_string());
    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Compiles an action script into a script with one function, [`ENTRY`].
pub fn compile_action(source: &str) -> Result<Script, String> {
    let mut b = Builder {
        code: Vec::new(),
        imports: Vec::new(),
        strings: BTreeMap::new(),
        pool_size: 0,
    };
    for statement in statements(source) {
        // Keywords are case-insensitive: the data writes `wait` and `Wait`.
        let keyword = |word: &str| {
            statement
                .get(..word.len())
                .filter(|head| head.eq_ignore_ascii_case(word))
                .map(|_| &statement[word.len()..])
                .filter(|rest| rest.is_empty() || rest.starts_with([' ', '{', '\t']))
        };
        // `yield` gives up the rest of a frame; an action has nothing to give
        // it to, so it compiles to nothing.
        if keyword("yield").is_some_and(str::is_empty) {
            continue;
        }
        let (waits, body) = match keyword("wait") {
            Some(rest) => (true, rest.trim()),
            None => (false, statement.as_str()),
        };
        let calls: Vec<String> = match body.strip_prefix('{') {
            Some(group) => statements(group.strip_suffix('}').ok_or("unclosed {")?),
            None if body.is_empty() => Vec::new(),
            None => vec![body.to_string()],
        };
        if waits {
            b.op(Op::BeginWait, 0);
        }
        for call in calls {
            let (name, args) = parse_call(&call).map_err(|e| format!("{call:?}: {e}"))?;
            b.call(&name, &args);
        }
        if waits {
            b.op(Op::EndWait, 0);
        }
    }
    b.op(Op::ReturnV, 0);
    Ok(Script {
        imports: b.imports,
        strings: b.strings,
        variables: Vec::new(),
        functions: vec![(ENTRY.to_string(), 0)],
        code: b.code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::decode_all;

    #[test]
    fn compiles_waited_and_plain_calls() {
        let script =
            compile_action(r#"wait CallSheep("lby102p","STAIRS_LEFT"); SetLocation("HAL");"#)
                .unwrap();
        let ops: Vec<Op> = decode_all(&script.code)
            .unwrap()
            .iter()
            .map(|i| i.op)
            .collect();
        assert_eq!(ops[0], Op::BeginWait);
        assert_eq!(
            ops.iter().filter(|&&o| o == Op::CallSysFunctionV).count(),
            2
        );
        assert_eq!(*ops.last().unwrap(), Op::ReturnV);
        assert_eq!(script.imports[1].name, "SetLocation");
    }

    #[test]
    fn groups_and_negative_numbers() {
        let script =
            compile_action("wait { TurnHead(\"GAB\", -20, 5, 500); SetTimerSeconds(1.5); }")
                .unwrap();
        assert_eq!(
            script.imports[0].arguments,
            [Type::String, Type::Int, Type::Int, Type::Int]
        );
        assert_eq!(script.imports[1].arguments, [Type::Float]);
    }

    #[test]
    fn keywords_ignore_case() {
        assert!(compile_action("Wait CallSheep(\"rc2\",\"stairsgabe\"); Yield;").is_ok());
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        assert!(compile_action("if (x) Foo();").is_err());
        assert!(compile_action("Foo(GetFlag(\"x\"));").is_err());
    }
}
