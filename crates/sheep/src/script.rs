//! Compiled Sheep scripts (`.SHP`).
//!
//! Layout follows G-Engine's `SheepScript::ParseFromData`: a header with
//! section offsets, then sections named `SysImports`, `StringConsts`,
//! `Variables`, `Functions` and `Code`. Length-prefixed names carry one more
//! byte (their NUL) than the length says.

use std::collections::BTreeMap;

use gk3_formats::reader::Reader;
use gk3_formats::{Error, Result};

use crate::Value;

/// Sheep's three value types, as import signatures and variables code them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    Void,
    Int,
    Float,
    String,
}

impl Type {
    fn from_code(code: i32) -> Result<Self> {
        Ok(match code {
            0 => Self::Void,
            1 => Self::Int,
            2 => Self::Float,
            3 => Self::String,
            other => return Err(Error::Format(format!("unknown Sheep type code {other}"))),
        })
    }
}

/// A system function the script calls, with the signature it was compiled
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub name: String,
    pub returns: Type,
    pub arguments: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: String,
    pub initial: Value,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Script {
    pub imports: Vec<Import>,
    /// String constants by their offset in the pool, which is how bytecode
    /// refers to them.
    pub strings: BTreeMap<i32, String>,
    pub variables: Vec<Variable>,
    /// Function names (with their `$`) and bytecode offsets, in file order.
    pub functions: Vec<(String, usize)>,
    pub code: Vec<u8>,
}

impl Script {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        if r.bytes(8)? != b"GK3Sheep" {
            return Err(Error::Format("not a compiled Sheep script".into()));
        }
        r.skip(4)?;
        let header_size = r.u32()? as usize;
        r.skip(8)?;
        let count = r.u32()? as usize;
        r.expect_room(count, 4)?;
        let offsets: Vec<usize> = (0..count)
            .map(|_| r.u32().map(|o| o as usize))
            .collect::<Result<_>>()?;

        let mut script = Script::default();
        for offset in offsets {
            r.seek(header_size + offset)?;
            let name = r.fixed_str(12)?;
            match name.as_str() {
                "SysImports" => script.imports = imports(&mut r)?,
                "StringConsts" => script.strings = strings(&mut r)?,
                "Variables" => script.variables = variables(&mut r)?,
                "Functions" => script.functions = functions(&mut r)?,
                "Code" => script.code = code(&mut r)?,
                other => return Err(Error::Format(format!("unknown Sheep section {other:?}"))),
            }
        }
        Ok(script)
    }

    pub fn function(&self, name: &str) -> Option<usize> {
        self.functions
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|&(_, offset)| offset)
    }

    pub fn string(&self, offset: i32) -> Option<&str> {
        self.strings.get(&offset).map(String::as_str)
    }
}

/// A 16-bit length, that many bytes, then the NUL the length leaves out.
fn name(r: &mut Reader) -> Result<String> {
    let len = r.u16()? as usize;
    let text = r.fixed_str(len)?;
    r.skip(1)?;
    Ok(text)
}

/// Sections repeat their sizes and an offset table before their entries.
fn entry_count(r: &mut Reader) -> Result<usize> {
    r.skip(12)?;
    let count = r.u32()? as usize;
    r.expect_room(count, 4)?;
    r.skip(4 * count)?;
    Ok(count)
}

fn imports(r: &mut Reader) -> Result<Vec<Import>> {
    let count = entry_count(r)?;
    (0..count)
        .map(|_| {
            let name = name(r)?;
            let returns = Type::from_code(r.u8()? as i8 as i32)?;
            let argc = r.u8()? as usize;
            let arguments = (0..argc)
                .map(|_| Type::from_code(r.u8()? as i8 as i32))
                .collect::<Result<_>>()?;
            Ok(Import {
                name,
                returns,
                arguments,
            })
        })
        .collect()
}

fn strings(r: &mut Reader) -> Result<BTreeMap<i32, String>> {
    r.skip(8)?;
    let size = r.u32()? as usize;
    let count = r.u32()? as usize;
    r.expect_room(count, 4)?;
    let offsets: Vec<usize> = (0..count)
        .map(|_| r.u32().map(|o| o as usize))
        .collect::<Result<_>>()?;
    let pool = r.bytes(size)?;
    let mut out = BTreeMap::new();
    for (i, &start) in offsets.iter().enumerate() {
        let end = offsets.get(i + 1).copied().unwrap_or(size);
        let raw = pool.get(start..end).ok_or_else(|| {
            Error::Format(format!("string {i} at {start}..{end} is outside the pool"))
        })?;
        let text = raw.split(|&b| b == 0).next().unwrap_or(&[]);
        out.insert(start as i32, gk3_formats::reader::latin1(text));
    }
    Ok(out)
}

fn variables(r: &mut Reader) -> Result<Vec<Variable>> {
    let count = entry_count(r)?;
    (0..count)
        .map(|_| {
            let name = name(r)?;
            let initial = match Type::from_code(r.i32()?)? {
                Type::Int => Value::Int(r.i32()?),
                Type::Float => Value::Float(r.f32()?),
                Type::String => {
                    r.skip(4)?;
                    Value::Str(String::new().into())
                }
                Type::Void => return Err(Error::Format(format!("variable {name} is void"))),
            };
            Ok(Variable { name, initial })
        })
        .collect()
}

fn functions(r: &mut Reader) -> Result<Vec<(String, usize)>> {
    let count = entry_count(r)?;
    (0..count)
        .map(|_| {
            let name = name(r)?;
            r.skip(2)?;
            Ok((name, r.u32()? as usize))
        })
        .collect()
}

fn code(r: &mut Reader) -> Result<Vec<u8>> {
    r.skip(8)?;
    let len = r.u32()? as usize;
    let count = r.u32()?;
    if count != 1 {
        return Err(Error::Format(format!("{count} code blocks, expected one")));
    }
    r.skip(4)?;
    Ok(r.bytes(len)?.to_vec())
}
