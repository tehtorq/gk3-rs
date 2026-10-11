//! Sheep expressions written in text: scene file conditions such as
//! `[MODELS={IsCurrentTime("202p") && GetEgoCurrentLocationCount() < 1}]`
//! and action file conditions.
//!
//! The grammar's expression rules: unary `-` and `!`; `* / %`; `+ -`;
//! comparisons `< > <= >= == = != <>`; `&&`; `||`; parentheses; calls with
//! argument lists; integer, float and string literals; identifiers, which a
//! resolver may answer (action files use `n$` and `v$`). Identifiers are
//! case-insensitive.

use std::rc::Rc;

use crate::vm::{CallContext, Host, Outcome};
use crate::{Type, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Value),
    Name(String),
    Call(String, Vec<Expr>),
    Unary(char, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Int(i32),
    Float(f32),
    Str(String),
    Ident(String),
    Op(&'static str),
}

const OPERATORS: [&str; 19] = [
    "&&", "||", "<=", ">=", "==", "!=", "<>", "<", ">", "=", "+", "-", "*", "/", "%", "!", "(",
    ")", ",",
];

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' {
            let end = chars[i + 1..]
                .iter()
                .position(|&c| c == '"')
                .ok_or("unterminated string")?;
            tokens.push(Token::Str(chars[i + 1..i + 1 + end].iter().collect()));
            i += end + 2;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            tokens.push(if text.contains('.') {
                Token::Float(text.parse().map_err(|_| format!("bad number {text}"))?)
            } else {
                Token::Int(text.parse().map_err(|_| format!("bad number {text}"))?)
            });
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            // A user identifier ends in `$`, with no space before it.
            if chars.get(i) == Some(&'$') {
                i += 1;
            }
            tokens.push(Token::Ident(chars[start..i].iter().collect()));
        } else {
            // Maximal munch: the longest operator that matches.
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op = OPERATORS
                .iter()
                .find(|op| rest.starts_with(**op))
                .ok_or_else(|| format!("unexpected {c:?}"))?;
            tokens.push(Token::Op(op));
            i += op.len();
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek_op(&self) -> Option<&'static str> {
        match self.tokens.get(self.at) {
            Some(Token::Op(op)) => Some(op),
            _ => None,
        }
    }

    fn eat(&mut self, op: &str) -> bool {
        if self.peek_op() == Some(op) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Binary levels, loosest first.
    fn level(&mut self, depth: usize) -> Result<Expr, String> {
        const LEVELS: [&[&str]; 5] = [
            &["||"],
            &["&&"],
            &["==", "=", "!=", "<>", "<", ">", "<=", ">="],
            &["+", "-"],
            &["*", "/", "%"],
        ];
        if depth == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.level(depth + 1)?;
        while let Some(op) = self.peek_op().filter(|op| LEVELS[depth].contains(op)) {
            self.at += 1;
            let right = self.level(depth + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("!") {
            return Ok(Expr::Unary('!', Box::new(self.unary()?)));
        }
        if self.eat("-") {
            return Ok(Expr::Unary('-', Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let token = self
            .tokens
            .get(self.at)
            .cloned()
            .ok_or("expression ends early")?;
        self.at += 1;
        Ok(match token {
            Token::Int(i) => Expr::Literal(Value::Int(i)),
            Token::Float(f) => Expr::Literal(Value::Float(f)),
            Token::Str(s) => Expr::Literal(Value::Str(s.into())),
            Token::Ident(name) => {
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.level(0)?);
                            if self.eat(")") {
                                break;
                            }
                            if !self.eat(",") {
                                return Err(format!("expected , or ) in call to {name}"));
                            }
                        }
                    }
                    Expr::Call(name, args)
                } else {
                    Expr::Name(name)
                }
            }
            Token::Op("(") => {
                let inner = self.level(0)?;
                if !self.eat(")") {
                    return Err("missing )".into());
                }
                inner
            }
            Token::Op(op) => return Err(format!("unexpected {op}")),
        })
    }
}

impl Expr {
    pub fn parse(text: &str) -> Result<Expr, String> {
        let mut parser = Parser {
            tokens: tokenize(text)?,
            at: 0,
        };
        let expr = parser.level(0)?;
        if parser.at != parser.tokens.len() {
            return Err(format!("unexpected text after the expression in {text:?}"));
        }
        Ok(expr)
    }

    /// Evaluates against a host. Calls are made as from a thread outside any
    /// wait block; `resolve` answers bare identifiers.
    pub fn eval(&self, host: &mut dyn Host, resolve: &dyn Fn(&str) -> Option<Value>) -> Value {
        match self {
            Expr::Literal(v) => v.clone(),
            Expr::Name(name) => resolve(name).unwrap_or(Value::Int(0)),
            Expr::Call(name, args) => {
                let args: Vec<Value> = args.iter().map(|a| a.eval(host, resolve)).collect();
                let context = CallContext {
                    thread: usize::MAX,
                    script: "",
                    in_wait: false,
                };
                match host.call(name, &args, Type::Int, &context) {
                    Outcome::Done(v) | Outcome::Pending(v) => v,
                    Outcome::Spawn { .. } => Value::Int(0),
                }
            }
            Expr::Unary('!', e) => Value::from(!e.eval(host, resolve).truthy()),
            Expr::Unary(_, e) => match e.eval(host, resolve) {
                Value::Float(f) => Value::Float(-f),
                v => Value::Int(-v.int()),
            },
            Expr::Binary(op, a, b) => {
                // Both sides short-circuit as in C.
                if *op == "&&" {
                    return Value::from(
                        a.eval(host, resolve).truthy() && b.eval(host, resolve).truthy(),
                    );
                }
                if *op == "||" {
                    return Value::from(
                        a.eval(host, resolve).truthy() || b.eval(host, resolve).truthy(),
                    );
                }
                binary(op, a.eval(host, resolve), b.eval(host, resolve))
            }
        }
    }
}

fn binary(op: &str, a: Value, b: Value) -> Value {
    // Strings compare as strings, ignoring case, like everything in Sheep.
    if let (Value::Str(x), Value::Str(y)) = (&a, &b) {
        let eq = x.eq_ignore_ascii_case(y);
        return match op {
            "==" | "=" => Value::from(eq),
            "!=" | "<>" => Value::from(!eq),
            _ => Value::Int(0),
        };
    }
    let float = matches!(a, Value::Float(_)) || matches!(b, Value::Float(_));
    if float {
        let (x, y) = (a.float(), b.float());
        return match op {
            "+" => Value::Float(x + y),
            "-" => Value::Float(x - y),
            "*" => Value::Float(x * y),
            "/" => Value::Float(if y == 0.0 { 0.0 } else { x / y }),
            "%" => Value::Int(a.int().checked_rem(b.int()).unwrap_or(0)),
            "==" | "=" => Value::from(x == y),
            "!=" | "<>" => Value::from(x != y),
            "<" => Value::from(x < y),
            ">" => Value::from(x > y),
            "<=" => Value::from(x <= y),
            ">=" => Value::from(x >= y),
            _ => Value::Int(0),
        };
    }
    let (x, y) = (a.int(), b.int());
    match op {
        "+" => Value::Int(x.wrapping_add(y)),
        "-" => Value::Int(x.wrapping_sub(y)),
        "*" => Value::Int(x.wrapping_mul(y)),
        "/" => Value::Int(x.checked_div(y).unwrap_or(0)),
        "%" => Value::Int(x.checked_rem(y).unwrap_or(0)),
        "==" | "=" => Value::from(x == y),
        "!=" | "<>" => Value::from(x != y),
        "<" => Value::from(x < y),
        ">" => Value::from(x > y),
        "<=" => Value::from(x <= y),
        ">=" => Value::from(x >= y),
        _ => Value::Int(0),
    }
}

/// Convenience for tests and simple hosts: a host answering calls with a
/// closure.
pub struct FnHost<F: FnMut(&str, &[Value]) -> Value>(pub F);

impl<F: FnMut(&str, &[Value]) -> Value> Host for FnHost<F> {
    fn call(&mut self, name: &str, args: &[Value], _: Type, _: &CallContext) -> Outcome {
        Outcome::Done((self.0)(name, args))
    }

    fn script(&mut self, _: &str) -> Option<Rc<crate::Script>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(text: &str) -> Value {
        let mut host = FnHost(|name: &str, args: &[Value]| match name {
            "IsCurrentTime" => Value::from(args[0].string().eq_ignore_ascii_case("110A")),
            "GetEgoCurrentLocationCount" => Value::Int(0),
            "GetFlag" => Value::Int(1),
            _ => Value::Int(0),
        });
        Expr::parse(text)
            .unwrap()
            .eval(&mut host, &|n| (n == "n$").then(|| Value::from("DOOR")))
    }

    #[test]
    fn scene_conditions() {
        assert_eq!(
            eval(r#"IsCurrentTime("110a") && GetEgoCurrentLocationCount() < 1"#),
            Value::Int(1)
        );
        assert_eq!(
            eval(r#"!IsCurrentTime("110a") ||IsCurrentTime("207a")"#),
            Value::Int(0)
        );
        assert_eq!(
            eval(r#"(IsCurrentTime("207a")==0) && (IsCurrentTime("307a")==0)"#),
            Value::Int(1)
        );
        assert_eq!(eval("GetFlag(\"x\") <> 0"), Value::Int(1));
    }

    #[test]
    fn arithmetic_precedence_and_types() {
        assert_eq!(eval("1 + 2 * 3"), Value::Int(7));
        assert_eq!(eval("(1 + 2) * 3"), Value::Int(9));
        assert_eq!(eval("7 % 4 - -1"), Value::Int(4));
        assert_eq!(eval("1 + 0.5"), Value::Float(1.5));
        assert_eq!(eval("5 / 0"), Value::Int(0));
    }

    #[test]
    fn identifiers_and_strings() {
        assert_eq!(eval(r#"n$ == "door""#), Value::Int(1));
    }

    #[test]
    fn errors_are_reported() {
        assert!(Expr::parse("IsCurrentTime(").is_err());
        assert!(Expr::parse("1 2").is_err());
        assert!(Expr::parse("\"open").is_err());
    }
}
