use std::error;
use std::fmt;
use std::io;

const HEADER: [u8; 12] = *b"VM/MAR_26/LE";

#[derive(Debug)]
pub enum Error {
    StackOverflow,
    StackUnderflow,
    DivByZero,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::StackOverflow => write!(f, "stack overflow"),
            Error::StackUnderflow => write!(f, "stack underflow"),
            Error::DivByZero => write!(f, "tried to divide by zero"),
        }
    }
}

impl error::Error for Error {}

pub type Value = i64;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Procedure {
    Nop = 0,
    Push,
    Add,
    Sub,
    Mult,
    Div,
    Jump,
    JumpIf,
    Dup,
}

struct ProcedureDefinition {
    proc: Procedure,
    name: &'static str,
    expects_operand: bool,
}

impl ProcedureDefinition {
    fn decode_str(s: &str) -> Option<Self> {
        for def in PROC_DEFS {
            if s == def.name {
                return Some(def);
            }
        }
        None
    }
}

const PROC_DEFS: [ProcedureDefinition; 9] = [
    ProcedureDefinition {
        proc: Procedure::Nop,
        name: "nop",
        expects_operand: false,
    },
    ProcedureDefinition {
        proc: Procedure::Push,
        name: "push",
        expects_operand: true,
    },
    ProcedureDefinition {
        proc: Procedure::Add,
        name: "add",
        expects_operand: false,
    },
    ProcedureDefinition {
        proc: Procedure::Sub,
        name: "sub",
        expects_operand: false,
    },
    ProcedureDefinition {
        proc: Procedure::Mult,
        name: "mult",
        expects_operand: false,
    },
    ProcedureDefinition {
        proc: Procedure::Div,
        name: "div",
        expects_operand: false,
    },
    ProcedureDefinition {
        proc: Procedure::Jump,
        name: "jmp",
        expects_operand: true,
    },
    ProcedureDefinition {
        proc: Procedure::JumpIf,
        name: "jif",
        expects_operand: true,
    },
    ProcedureDefinition {
        proc: Procedure::Dup,
        name: "dup",
        expects_operand: false,
    },
];

impl Procedure {
    fn decode(b: u8) -> Option<Self> {
        const LEN: u8 = if PROC_DEFS.len() > u8::max_value() as usize {
            u8::max_value()
        } else {
            PROC_DEFS.len() as u8
        };
        match b {
            0..LEN => Some(PROC_DEFS[b as usize].proc),
            _ => None,
        }
    }
}

/// NOTE We don't make some Procedures carry an operand because in the future we
/// want to try to implement a #![no_std] version of this vm. Therefore if we
/// zero out some instruction space (future implementation of vm's debug mode
/// with a static stack size), all instructions automatically get set to Nop,
/// which catches runtime bugs and makes the machine panic, instead of producing
/// weird side-effects.
///
/// TODO check if we really need this repr(C) and _pad. rustc might
/// automatically implement it for us.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instruction {
    proc: Procedure,
    _pad: [u8; 7],
    /// Operand is ignored for some operands. Errors inside Assembler or simply
    /// ignored when cast from bytecode.
    operand: Value,
}

impl Instruction {
    pub fn new(proc: Procedure, operand: Value) -> Self {
        Self {
            proc,
            _pad: [0u8; 7],
            operand,
        }
    }
}

/// We have two cases in which we want to create a machine:
/// 1. From IR (i.e we already have a program in Vector of array form):
/// ```rs
/// let mut stack = [0u8; STACK_SIZE];
/// let mut m = Machine::from_ir(&mut stack, &ir.as_slice())?;
/// ```
///
/// where ir is of type Vec<Instruction>.
///
/// 2. From binary file:
/// ```rs
/// let mut stack = [0u8; STACK_SIZE];
/// let mut program_bytes = read_binary_from_file();
/// let mut m = Macalign_of, size_bytes(&mut stack, &mut program_bytes)?;
/// ```
///
/// This way we cast the binary file directly to a `&[Instruction]`, therefore
/// eliminating the need for parsing assembly.
///
/// TODO Of course we want to also be able to parse direct assembly, therefore
/// making an Assembler structure needed and an ergonomic Machine::assembler()
/// wanted.
pub struct Machine<'m> {
    pub stack: &'m mut [Value],
    pub head: usize,
    pub program: &'m [Instruction],
    pub ip: usize,
}

#[derive(Debug)]
pub enum CastError {
    IndivisibleSize,
    Unaligned,
    UncastableType,
}

impl fmt::Display for CastError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CastError::IndivisibleSize => write!(f, "indivisible size"),
            CastError::Unaligned => write!(f, "unaligned"),
            CastError::UncastableType => write!(f, "tried to cast to type with no size"),
        }
    }
}

impl error::Error for CastError {}

fn check_castable<F, T>(source: &[F]) -> Result<usize, CastError> {
    use std::mem::{size_of, align_of};
    if size_of::<T>() == 0 {
        return Err(CastError::UncastableType);
    }

    if source.len() % size_of::<T>() != 0 {
        return Err(CastError::IndivisibleSize);
    }

    let ptr = source.as_ptr();
    if ptr.align_offset(align_of::<T>()) != 0 {
        return Err(CastError::Unaligned);
    }

    Ok(source.len() / std::mem::size_of::<T>())
}

fn cast_slice_mut<F, T>(source: &mut [F]) -> Result<&mut [T], CastError> {
    let len = check_castable::<F, T>(source)?;
    let ptr = source.as_mut_ptr();
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut T, len) })
}

impl<'a> Machine<'a> {
    pub fn new(stack: &'a mut [Value], program: &'a [Instruction]) -> Self {
        Self { stack, head: 0, program, ip: 0 }
    }

    pub fn from_ir(stack: &'a mut [u8], program: &'a [Instruction]) -> Result<Self, CastError> {
        let stack = cast_slice_mut::<u8, Value>(stack)?;
        Ok(Self::new(stack, program))
    }

    pub fn from_reader<R: io::Read>(r: R, stack: &'a mut [u8], program: &'a mut [Instruction]) -> Result<Self, LoadError> {
        let n = load_program(r, program)?;
        let ir = &program[0..n];
        Self::from_ir(stack, ir).map_err(|e| LoadError::CastError(e))
    }

    fn next_instruction(&mut self) -> Option<Instruction> {
        if self.ip >= self.program.len() {
            None
        } else {
            let popped = self.program[self.ip];
            self.ip += 1;
            Some(popped)
        }
    }

    fn push_stack(&mut self, v: Value) -> Result<(), Error> {
        if self.head >= self.stack.len() {
            Err(Error::StackOverflow)
        } else {
            self.stack[self.head] = v;
            self.head += 1;
            Ok(())
        }
    }

    fn pop_stack(&mut self) -> Result<Value, Error> {
        if self.head == 0 {
            Err(Error::StackUnderflow)
        } else {
            self.head -= 1;
            Ok(self.stack[self.head])
        }
    }

    pub fn last_value(&self) -> Option<Value> {
        if self.head == 0 {
            None
        } else {
            Some(self.stack[self.head - 1])
        }
    }

    /// The machine halts when `ip` reaches the end of the program.
    pub fn run(&mut self) -> Result<(), Error> {
        use Procedure::*;
        loop {
            let ins = match self.next_instruction() {
                Some(i) => i,
                None => return Ok(()),
            };

            macro_rules! binary_op {
                ($op:tt) => {
                    {
                        let b = self.pop_stack()?;
                        let a = self.pop_stack()?;
                        self.push_stack(a $op b)?;
                    }
                };
            }

            match ins.proc {
                Nop => continue,
                Push => self.push_stack(ins.operand)?,
                Add => binary_op!(+),
                Sub => binary_op!(-),
                Mult => binary_op!(*),
                Div => {
                    let b = self.pop_stack()?;
                    if b == 0 {
                        return Err(Error::DivByZero);
                    }
                   let a = self.pop_stack()?;
                    self.push_stack(a / b)?;
                },
                Jump => self.ip = ins.operand as usize,
                JumpIf => {
                    let cond = self.pop_stack()?;
                    if cond > 0 {
                       self.ip = ins.operand as usize;
                    }
                },
                Dup => self.push_stack(self.last_value().ok_or(Error::StackUnderflow)?)?,
            }
        }
    }

    /// Helper method to help discoverability of Assembler
    #[allow(dead_code)]
    pub fn assembler() -> Assembler {
        Assembler::new()
    }
}

pub fn save_program<W: io::Write>(mut w: W, program: &[Instruction]) -> io::Result<()> {
    w.write_all(&HEADER)?;
    let count: u32 = program.len().try_into().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "program too large")
    })?;
    w.write_all(&count.to_le_bytes())?;

    for ins in program {
        // 9 bytes per instruction
        w.write_all(&[ins.proc as u8])?;
        w.write_all(&ins.operand.to_le_bytes())?;
    }

    Ok(())
}

#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    CastError(CastError),
    FaultyHeader,
    BackingTooSmall,
    Truncated,
    UnknownProcedure,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "{}", e),
            LoadError::CastError(e) => write!(f, "{}", e),
            LoadError::FaultyHeader => write!(f, "faulty header"),
            LoadError::BackingTooSmall => write!(f, "program's backing buffer is too small"),
            LoadError::Truncated => write!(f, "instruction got cut of by eof"),
            LoadError::UnknownProcedure => write!(f, "unknown procedure-byte"),
        }
    }
}

impl error::Error for LoadError {}

impl From<io::Error> for LoadError {
    fn from(e: io::Error) -> Self { LoadError::Io(e) }
}

fn load_program<R: io::Read>(mut r: R, out: &mut [Instruction]) -> Result<usize, LoadError> {
    let mut  header = [0u8; HEADER.len()];
    r.read_exact(&mut header)?;
    if header != HEADER {
        return Err(LoadError::FaultyHeader);
    }

    let mut count_bytes = [0u8; 4];
    r.read_exact(&mut count_bytes)?;
    let count = u32::from_le_bytes(count_bytes) as usize;

    if count > out.len() {
        return Err(LoadError::BackingTooSmall);
    }

    for i in 0..count {
        let mut proc_b = [0u8; 1];
        let mut oper_b = [0u8; 8];

        r.read_exact(&mut proc_b)
            .map_err(|e| if e.kind() == io::ErrorKind::UnexpectedEof {
                LoadError::Truncated
            } else { LoadError::Io(e) })?;

        r.read_exact(&mut oper_b)
            .map_err(|e| if e.kind() == io::ErrorKind::UnexpectedEof {
                LoadError::Truncated
            } else { LoadError::Io(e) })?;

        let proc = Procedure::decode(proc_b[0]);
        if proc.is_none() {
            return Err(LoadError::UnknownProcedure);
        }
        let proc = proc.unwrap();

        out[i] = Instruction::new(proc, Value::from_le_bytes(oper_b));
    }

    Ok(count)
}

pub struct Assembler {
    line_no: u32,
}

#[derive(Debug, PartialEq)]
pub struct AsmError {
    row: u32,
    msg: String,
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.row, self.msg)
    }
}

impl Assembler {
    pub fn new() -> Self {
        Self{ line_no: 0 }
    }

    pub fn assemble_str(&mut self, source: &str) -> Result<Vec<Instruction>, AsmError> {
        self.assemble(std::io::Cursor::new(source))
    }

    pub fn assemble<R: io::BufRead>(&mut self, reader: R) -> Result<Vec<Instruction>, AsmError> {
        let mut program = Vec::new();

        for l in reader.lines() {
            self.line_no += 1;

            let l = l.map_err(|e| self.err(format!("io error: {}", e)))?;
            let mut split = l.split_whitespace();

            let proc_str = match split.next() {
                Some(p) if p == ";" => continue,
                Some(p) => p,
                None => continue, // empty line
            };

            let proc_def = ProcedureDefinition::decode_str(proc_str)
                .ok_or(self.err(format!("unknown procedure: '{proc_str}'")))?;

            let op_tok = match split.next() {
                Some(";") | None => None,
                Some(v) => Some(v),
            };
            let (op_str, op_val) = match op_tok {
                Some(v) => (v, Value::from_str_radix(v, 10).ok()),
                None => ("", None),
            };

            let op = match (proc_def.expects_operand, op_tok, op_val) {
                (true, _, Some(i)) => i,
                (true, _, None) => {
                    let msg = format!(
                        "procedure '{}' expects an integer operand, found: '{}'",
                        proc_str, op_str
                    );
                    return Err(self.err(msg));
                }
                (false, Some(v), _) => {
                    let msg = format!("procedure '{}' expects no operand, found: '{}'", proc_str, v);
                    return Err(self.err(msg));
                }
                (false, None, _) => 0,
            };

            let ins = Instruction::new(proc_def.proc, op);
            program.push(ins);
        }

        Ok(program)
    }

    fn err(&self, msg: String) -> AsmError {
        AsmError{ row: self.line_no, msg }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM_CAPACITY: usize = 64 * 1024;

    #[test]
    fn assemble() {
        let source = "
            push 4
            push 1
            push 11  ; [4 1 11]
            add      ; [4 12]
            mult     ; [48]
            push 4
            sub      ; [44]
            push 4
            div      ; [11]
            ";
        let mut asm = Assembler::new();
        let res = asm.assemble_str(source);
        if let Err(e) = &res {
            println!("error ({}): {}", e.row, e.msg);
            assert!(false);
        }

        let parsed = res.ok().unwrap();

        let mut program = Vec::new();
        program.push(Instruction::new(Procedure::Push, 4));
        program.push(Instruction::new(Procedure::Push, 1));
        program.push(Instruction::new(Procedure::Push, 11));
        program.push(Instruction::new(Procedure::Add, 0));
        program.push(Instruction::new(Procedure::Mult, 0));
        program.push(Instruction::new(Procedure::Push, 4));
        program.push(Instruction::new(Procedure::Sub, 0));
        program.push(Instruction::new(Procedure::Push, 4));
        program.push(Instruction::new(Procedure::Div, 0));

        assert_eq!(parsed.len(), program.len());
        for (got, exp) in parsed.iter().zip(program) {
            assert_eq!(got.clone(), exp);
        }

        let source = "
            push 4
            push 1
            push 11  ; [4 1 11]
            add  1   ; [4 12]
            mult     ; [48]
            push 4
            sub      ; [44]
            push 4
            div      ; [11]
            ";
        let mut asm = Assembler::new();
        let res = asm.assemble_str(source);
        assert_eq!(res, Err(AsmError{ row: 5, msg: "procedure 'add' expects no operand, found: '1'".to_string() }));

        let source = "
            push 4
            push 1
            push 11  ; [4 1 11]
            add  a   ; [4 12]
            mult     ; [48]
            push 4
            sub      ; [44]
            push 4
            div      ; [11]
            ";
        let mut asm = Assembler::new();
        let res = asm.assemble_str(source);
        assert_eq!(res, Err(AsmError{ row: 5, msg: "procedure 'add' expects no operand, found: 'a'".to_string() }));
    }

    #[test]
    fn simple_run() {
        let mut program = Vec::new();

        program.push(Instruction::new(Procedure::Push, 4));
        program.push(Instruction::new(Procedure::Push, 1));
        program.push(Instruction::new(Procedure::Push, 11));
        // [4 1 11]

        program.push(Instruction::new(Procedure::Add, 0));
        // [4 12]

        program.push(Instruction::new(Procedure::Mult, 0));
        // [48]

        program.push(Instruction::new(Procedure::Push, 4));
        program.push(Instruction::new(Procedure::Sub, 0));
        // [44]

        program.push(Instruction::new(Procedure::Push, 4));
        program.push(Instruction::new(Procedure::Div, 0));
        // [11]

        let mut stack = [0u8; MEM_CAPACITY];

        let mut m = Machine::from_ir(&mut stack, program.as_slice()).unwrap();
        assert_eq!(m.head, 0);
        assert_eq!(m.ip, 0);

        m.run().unwrap();
        let res = m.last_value().unwrap();
        assert_eq!(res, 11);
    }

    // TODO implement fibonacii test
}
