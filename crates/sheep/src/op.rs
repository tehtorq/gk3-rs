//! The instruction set: 52 opcodes, 0x00 to 0x34, with 0x0C unused.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    SitnSpin = 0x00,
    Yield,
    CallSysFunctionV,
    CallSysFunctionI,
    CallSysFunctionF,
    CallSysFunctionS,
    Branch,
    BranchGoto,
    BranchIfZero,
    BeginWait,
    EndWait,
    ReturnV,
    StoreI = 0x0D,
    StoreF,
    StoreS,
    LoadI,
    LoadF,
    LoadS,
    PushI,
    PushF,
    PushS,
    Pop,
    AddI,
    AddF,
    SubtractI,
    SubtractF,
    MultiplyI,
    MultiplyF,
    DivideI,
    DivideF,
    NegateI,
    NegateF,
    IsEqualI,
    IsEqualF,
    IsNotEqualI,
    IsNotEqualF,
    IsGreaterI,
    IsGreaterF,
    IsLessI,
    IsLessF,
    IsGreaterEqualI,
    IsGreaterEqualF,
    IsLessEqualI,
    IsLessEqualF,
    IToF,
    FToI,
    Modulo,
    And,
    Or,
    Not,
    GetString,
    DebugBreakpoint,
}

impl Op {
    pub fn from_byte(b: u8) -> Option<Self> {
        use Op::*;
        const TABLE: [Option<Op>; 0x35] = [
            Some(SitnSpin),
            Some(Yield),
            Some(CallSysFunctionV),
            Some(CallSysFunctionI),
            Some(CallSysFunctionF),
            Some(CallSysFunctionS),
            Some(Branch),
            Some(BranchGoto),
            Some(BranchIfZero),
            Some(BeginWait),
            Some(EndWait),
            Some(ReturnV),
            None,
            Some(StoreI),
            Some(StoreF),
            Some(StoreS),
            Some(LoadI),
            Some(LoadF),
            Some(LoadS),
            Some(PushI),
            Some(PushF),
            Some(PushS),
            Some(Pop),
            Some(AddI),
            Some(AddF),
            Some(SubtractI),
            Some(SubtractF),
            Some(MultiplyI),
            Some(MultiplyF),
            Some(DivideI),
            Some(DivideF),
            Some(NegateI),
            Some(NegateF),
            Some(IsEqualI),
            Some(IsEqualF),
            Some(IsNotEqualI),
            Some(IsNotEqualF),
            Some(IsGreaterI),
            Some(IsGreaterF),
            Some(IsLessI),
            Some(IsLessF),
            Some(IsGreaterEqualI),
            Some(IsGreaterEqualF),
            Some(IsLessEqualI),
            Some(IsLessEqualF),
            Some(IToF),
            Some(FToI),
            Some(Modulo),
            Some(And),
            Some(Or),
            Some(Not),
            Some(GetString),
            Some(DebugBreakpoint),
        ];
        TABLE.get(b as usize).copied().flatten()
    }

    /// Whether a 32-bit operand follows the opcode.
    pub fn has_operand(self) -> bool {
        use Op::*;
        matches!(
            self,
            CallSysFunctionV
                | CallSysFunctionI
                | CallSysFunctionF
                | CallSysFunctionS
                | Branch
                | BranchGoto
                | BranchIfZero
                | StoreI
                | StoreF
                | StoreS
                | LoadI
                | LoadF
                | LoadS
                | PushI
                | PushF
                | PushS
                | IToF
                | FToI
        )
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// One decoded instruction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instruction {
    pub at: usize,
    pub op: Op,
    pub operand: i32,
}

/// Decodes one instruction at `at`.
pub fn decode(code: &[u8], at: usize) -> Result<Instruction, String> {
    let byte = *code
        .get(at)
        .ok_or_else(|| format!("no instruction at {at}"))?;
    let op = Op::from_byte(byte).ok_or_else(|| format!("unknown opcode {byte:#04x} at {at}"))?;
    let operand = if op.has_operand() {
        let bytes = code
            .get(at + 1..at + 5)
            .ok_or_else(|| format!("{op} at {at} is missing its operand"))?;
        i32::from_le_bytes(bytes.try_into().unwrap())
    } else {
        0
    };
    Ok(Instruction { at, op, operand })
}

impl Instruction {
    pub fn len(&self) -> usize {
        if self.op.has_operand() { 5 } else { 1 }
    }

    pub fn is_empty(&self) -> bool {
        false
    }
}

/// Decodes a whole code block from start to end.
pub fn decode_all(code: &[u8]) -> Result<Vec<Instruction>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let i = decode(code, at)?;
        at += i.len();
        out.push(i);
    }
    Ok(out)
}
