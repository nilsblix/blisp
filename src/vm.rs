pub type Word = i64;

const _: () = {
    assert!(size_of::<Word>() % size_of::<u8>() == 0);
    // TODO Check size of Op. We want to be able to translate an entire program
    // into bytecode, therefore byte-divisibility is important for
    // tightly-packed bytecode.
};

#[derive(Clone, Copy)]
pub enum Binop {
    Add,
    Sub,
    Mult,
    Div,
    Mod,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    BitOr,
    BitAnd,
}

/// Labels are stored as u64, but are not part of the bytecode. Label
/// "instructions" only exist during parsing, and are therefore not part of the
/// instruction-set.
#[derive(Clone, Copy)]
pub enum Op {
    Nop,
    Push(Word),
    Call(u64),
    Binop(Binop),
    Swap,
    Over,
    /// Jump to label.
    Jump(u64),
    /// Jump to label, if head of stack is non zero.
    JumpIfNonZero(u64),
    Return,
}

pub struct Func<'o> {
    ops: &'o [Op],
    /// (label, op_idx)
    labels: Vec<(u64, usize)>,
    name: u64,
}

impl <'o> Func<'o> {
    pub fn next_op(&self, ip: &mut usize) -> Option<Op> {
        if *ip == self.ops.len() {
            None
        } else {
            let ret = self.ops[*ip];
            *ip += 1;
            Some(ret)
        }
    }

    pub fn match_label(&self, lab: u64) -> Option<usize> {
        for (label, op_idx) in self.labels.as_slice() {
            if *label == lab {
                return Some(*op_idx);
            }
        }
        None
    }
}

pub struct Machine<'m> {
    stack: Vec<Word>,
    entry: &'m Func<'m>,
    funcs: &'m [Func<'m>],
}

impl<'m> Machine<'m> {
    pub fn new(entry: &'m Func<'m>, funcs: &'m [Func<'m>]) -> Self {
        Self { stack: Vec::new(), entry, funcs }
    }

    pub fn match_func(&self, name: u64) -> Option<&'m Func<'m>> {
        for p in self.funcs {
            if p.name == name {
                return Some(p);
            }
        }

        None
    }

    pub fn run(&mut self) -> Result<Word, &'static str> {
        macro_rules! binary_op {
            ($op:tt) => {
                {
                    let b = self.stack.pop().ok_or("stack underflow")?;
                    let a = self.stack.pop().ok_or("stack underflow")?;
                    self.stack.push(a $op b);
                }
            };
        }

        macro_rules! binary_cmp {
            ($op:tt) => {
                {
                    let rhs = self.stack.pop().ok_or("stack underflow")?;
                    let lhs = self.stack.pop().ok_or("stack underflow")?;
                    if lhs $op rhs {
                        self.stack.push(1);
                    } else {
                        self.stack.push(0);
                    }
                }
            };
        }

        let func = self.entry;
        let mut ip = 0;

        loop {
            let op = func
                .next_op(&mut ip)
                .ok_or("malformed function: doesn't contain return instruction")?;
            match op {
                Op::Nop => continue,
                Op::Push(w) => self.stack.push(w),
                Op::Call(name) => {
                    self.entry = self.match_func(name)
                        .ok_or("tried to call unknown function")?;
                    let res = self.run()?;
                    self.stack.push(res);
                },
                Op::Binop(b) => match b {
                    Binop::Add  => binary_op!(+),
                    Binop::Sub  => binary_op!(-),
                    Binop::Mult => binary_op!(*),
                    Binop::Div  => {
                        let b = self.stack.pop().ok_or("stack underflow")?;
                        if b == 0 {
                            return Err("tried to div by zero");
                        }
                        let a = self.stack.pop().ok_or("stack underflow")?;
                        self.stack.push(a / b);
                    },
                    Binop::Mod          => binary_op!(%),
                    Binop::Equal        => binary_cmp!(==),
                    Binop::NotEqual     => binary_cmp!(!=),
                    Binop::Less         => binary_cmp!(<),
                    Binop::LessEqual    => binary_cmp!(<=),
                    Binop::Greater      => binary_cmp!(>),
                    Binop::GreaterEqual => binary_cmp!(>=),
                    Binop::BitOr        => binary_op!(|),
                    Binop::BitAnd       => binary_op!(&),
                },
                Op::Swap => {
                    let b = self.stack.pop().ok_or("stack underflow")?;
                    let a = *self.stack.last().ok_or("stack underflow")?;
                    *self.stack.last_mut().unwrap() = b;
                    self.stack.push(a);
                },
                Op::Over => {
                    let len = self.stack.len();
                    if len < 2 {
                        return Err("stack underflow");
                    }
                    self.stack.push(self.stack[len - 2]);
                },
                Op::Jump(lab) => ip = func.match_label(lab)
                    .ok_or("tried to jump to unknown label")?,
                Op::JumpIfNonZero(lab) => {
                    let cond = self.stack.pop().ok_or("stack underflow")?;
                    if cond != 0 {
                        ip = func.match_label(lab)
                            .ok_or("tried to jump to unknown label")?;
                    }
                },
                Op::Return => return Ok(self.stack.pop().ok_or("stack underflow")?),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_simple_machine() {
        let main = Func {
            ops: &[
                Op::Push(5),
                Op::Push(4),
                Op::Binop(Binop::Add),
                Op::Push(11),
                Op::Binop(Binop::Mult),
                Op::Return,
            ],
            labels: Vec::new(),
            name: 0,
        };

        let funcs = Vec::new();
        let mut m = Machine::new(&main, funcs.as_slice());
        let w = m.run().unwrap();
        assert_eq!(w, 99);
    }

    #[test]
    fn simple_fib() {
        let main = Func {
            ops: &[
                Op::Push(0),
                Op::Push(1),
                // label 0
                // FIXME probably need to dupe the fib number.
                Op::Push(1000),
                Op::Binop(Binop::Greater),
                Op::JumpIfNonZero(1),
                Op::Swap,
                Op::Over,
                Op::Binop(Binop::Add),
                Op::Jump(0),
                // label 1
                Op::Return,
            ],
            labels: vec![(0, 2), (1, 9)],
            name: 0,
        };

        let funcs = Vec::new();
        let mut m = Machine::new(&main, funcs.as_slice());
        let w = m.run().unwrap();
        assert_eq!(w, 987);
    }
}
