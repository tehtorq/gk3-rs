//! Sheep, GK3's scripting language.
//!
//! The game's logic ships as 224 compiled scripts. This crate reads them
//! ([`script`]), runs them ([`vm`]) against a [`Host`] that supplies the
//! game's API, and evaluates the Sheep expressions written in scene and
//! action files ([`expr`]).

pub mod compile;
pub mod expr;
pub mod op;
pub mod script;
pub mod vm;

use std::fmt;
use std::rc::Rc;

pub use script::{Script, Type};
pub use vm::{Host, Outcome, ThreadId, ThreadState, Vm};

/// A Sheep value. Strings are shared because they are copied around freely.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    Str(Rc<str>),
}

impl Value {
    pub fn int(&self) -> i32 {
        match self {
            Value::Int(i) => *i,
            Value::Float(f) => *f as i32,
            Value::Str(s) => s.trim().parse().unwrap_or(0),
        }
    }

    pub fn float(&self) -> f32 {
        match self {
            Value::Int(i) => *i as f32,
            Value::Float(f) => *f,
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
        }
    }

    pub fn string(&self) -> Rc<str> {
        match self {
            Value::Str(s) => s.clone(),
            Value::Int(i) => i.to_string().into(),
            Value::Float(f) => f.to_string().into(),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
        }
    }

    /// Converts to a declared type, as the machine does for call arguments.
    pub fn to_type(&self, ty: Type) -> Value {
        match ty {
            Type::Int | Type::Void => Value::Int(self.int()),
            Type::Float => Value::Float(self.float()),
            Type::String => Value::Str(self.string()),
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.into())
    }
}

impl From<i32> for Value {
    fn from(i: i32) -> Self {
        Value::Int(i)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Int(b as i32)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Str(s) => write!(f, "{s:?}"),
        }
    }
}

#[cfg(test)]
mod tests;
