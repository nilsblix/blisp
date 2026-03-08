use std::fmt;
use std::io::{self, Read, Write};
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

#[derive(Debug)]
pub enum ParseError {
    Eof,
    Lisp(LispError),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ParseError::*;
        match self {
            Eof => write!(f, "eof"),
            Lisp(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Debug)]
pub enum LispError {
    Parse(String),
    Type(String),
    Env(String),
    Prim(String),
}

impl fmt::Display for LispError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use LispError::*;
        match self {
            Parse(e) => write!(f, "parse: {e}"),
            Type(e) => write!(f, "type: {e}"),
            Env(e) => write!(f, "env: {e}"),
            Prim(e) => write!(f, "{e}"),
        }
    }
}

fn symbol_start(c: char) -> bool {
    match c {
        '*'|'/'|'>'|'<'|'='|'?'|'!'|'-'|'+'|'A'..='Z'|'a'..='z' => true,
        _ => false,
    }
}

pub struct Stream<S>
where S: Iterator<Item = char>
{
    s: std::iter::Peekable<S>,
    line_num: usize,
    unread: Vec<char>,
}

impl<S> Stream<S>
where S: Iterator<Item = char>
{
    pub fn new(s: S) -> Self {
        Self { s: s.peekable(), line_num: 1, unread: Vec::new() }
    }

    fn peek_char(&mut self) -> Option<char> {
        if let Some(c) = self.unread.last() {
            Some(*c)
        } else {
            self.s.peek().map(|c| *c)
        }
    }

    fn read_char(&mut self) -> Option<char> {
        let c = if let Some(c) = self.unread.pop() {
            c
        } else {
            match self.s.next() {
                Some(c) => c,
                None => return None,
            }
        };
        if c == '\n' {
            self.line_num += 1;
        }
        Some(c)
    }

    fn unread_char(&mut self, c: char) {
        if c == '\n' {
            self.line_num -= 1;
        }
        self.unread.push(c);
    }

    fn eat_whitespace(&mut self) {
        while let Some(c) = self.peek_char() {
            if c.is_whitespace() {
                _ = self.read_char();
            } else {
                break;
            }
        }
    }

    fn eat_comment(&mut self) {
        while let Some(c) = self.read_char() {
            if c == '\n' {
                break;
            }
        }
    }

    fn read_fixnum(&mut self, first: char) -> Result<Value, ParseError> {
        assert!(first == '-' || first.is_digit(10));
        let is_negative = first == '-';

        let mut acc: i64 = if is_negative { 0 } else { (first as u8 - b'0') as i64 };
        while let Some(c) = self.peek_char() {
            if let Some(d) = c.to_digit(10) {
                _ = self.read_char();
                acc = acc * 10 + d as i64;
            } else {
                break;
            }
        }

        if is_negative {
            acc *= -1;
        }

        Ok(Value::Fixnum(acc))
    }

    /// None means eof
    pub fn read_value(&mut self) -> Result<Value, ParseError> {
        self.eat_whitespace();

        let c = match self.read_char() {
            Some(c) => c,
            None => return Err(ParseError::Eof),
        };

        if c == ';' {
            self.eat_comment();
            return self.read_value();
        }

        if c.is_ascii_digit() || c == '~' {
            return self.read_fixnum(if c == '~' { '-' } else { c });
        }

        if symbol_start(c) {
            let mut acc = c.to_string();
            loop {
                if let Some(nc) = self.read_char() {
                    let is_delim = match nc {
                        '"'|'('|')'|'{'|'}'|';' => true,
                        nc => nc.is_whitespace(),
                    };
                    if is_delim {
                        self.unread_char(nc);
                        break;
                    } else {
                        acc.push(nc);
                        continue;
                    }
                }

                break;
            }
            return Ok(Value::Symbol(acc));
        }

        if c == '#' {
            match self.read_char() {
                Some('t') => return Ok(Value::Bool(true)),
                Some('f') => return Ok(Value::Bool(false)),
                Some(_) | None => { },
            }
        }

        if c == '(' {
            let mut acc = Value::Nil;
            loop {
                self.eat_whitespace();
                let nc = self.read_char();
                if nc.is_none() {
                    return Err(ParseError::Lisp(LispError::Parse("unexpected eof in list".to_string())));
                }

                let nc = nc.unwrap();

                if nc == ')' {
                    return reverse_list(acc) .map_err(|e| ParseError::Lisp(LispError::Parse(e)));
                }

                self.unread_char(nc);
                let car = self.read_value()?;
                acc = Value::Pair(Box::new((car, acc)));
            }
        }

        if c == '\'' {
            return Ok(Value::Quote(Box::new(self.read_value()?)));
        }

        let s = format!("unexpected char: {}", c);
        Err(ParseError::Lisp(LispError::Parse(s)))
    }
}

impl Stream<std::vec::IntoIter<char>> {
    pub fn from_str(s: &str) -> Self {
        let iter = s
            .chars()
            .collect::<Vec<char>>()
            .into_iter();
        Self::new(iter)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Env {
    items: Vec<(String, Rc<RefCell<Option<Value>>>)>,
}

#[derive(Debug)]
pub enum StdlibError {
    Io(io::Error),
    Lisp(LispError),
}

impl fmt::Display for StdlibError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use StdlibError::*;
        write!(f, "stdlib: ")?;
        match self {
            Io(e) => write!(f, "{e}"),
            Lisp(e) => write!(f, "{e}"),
        }
    }
}

impl Env {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn bind_rc(mut self, name: String, b: Rc<RefCell<Option<Value>>>) -> Self {
        // This optimization doesn't produce weird shadowing effects in 'let' due to let creating a
        // temporary env, therefore simply overwriting the last value.
        if let Some(item) = self.items.last_mut() {
            if item.0 == name {
                item.1 = b;
                return self;
            }
        }
        self.items.push((name, b));
        self
    }

    fn bind(self, name: String, value: Value) -> Self {
        self.bind_rc(name, Rc::new(RefCell::new(Some(value))))
    }

    fn make_loc() -> Rc<RefCell<Option<Value>>> {
        Rc::new(RefCell::new(None))
    }

    fn bind_list(mut self, bindings: Vec<(String, Value)>) -> Self {
        for (n, v) in bindings.iter() {
            self = self.bind(n.clone(), v.clone());
        }
        self
    }

    fn lookup(&self, name: &str) -> Result<Value, LispError> {
        let rm = self.lookup_mut(name)?;
        match rm.as_ref() {
            Some(lo) => Ok(lo.clone()),
            None => Err(LispError::Env(format!(
                "'{}' evaluated to an unspecified value in env",
                name
            ))),
        }
    }

    fn lookup_mut(&self, name: &str) -> Result<RefMut<'_, Option<Value>>, LispError> {
        for (n, cell) in self.items.iter().rev() {
            if n == name {
                return Ok(cell.borrow_mut());
            }
        }

        let s = format!("could not find '{}' in env", name);
        return Err(LispError::Env(s));
    }

    fn basis() -> Env {
        use Value::Primitive;
        fn num_args(name: &str, n: usize, args: &[&Value]) -> Result<(), LispError> {
            if args.len() != n {
                let s = format!("'{}' primitive expects '{}' number of arguments, found, '{}'" ,
                    name, n, args.len());
                return Err(LispError::Type(s));
            }

            Ok(())
        }

        macro_rules! bin_fixnum_prim {
            ($name:literal, $ctor:path, $op:tt) => {
                Primitive($name.to_string(), |args| {
                    num_args($name, 2, args)?;
                    if let (Value::Fixnum(a), Value::Fixnum(b)) = (args[0], args[1]) {
                        Ok($ctor(a $op b))
                    } else {
                        let s = format!("'{}' primitive expects integer arguments, found '{}' and '{}'",
                            $name, args[0], args[1]);
                        Err(LispError::Type(s))
                    }
                })
            };
        }

        let prim_eq = Primitive("eq".to_string(), |args| {
            num_args("eq", 2, args)?;
            Ok(Value::Bool(args[0] == args[1]))
        });

        let prim_mod = Primitive("mod".to_string(), |args| {
            num_args("mod", 2, args)?;
            if let (Value::Fixnum(a), Value::Fixnum(b)) = (args[0], args[1]) {
                Ok(Value::Fixnum(a % b))
            } else {
                let s = format!("'mod' primitive expects two integer arguments, found: '{}' and '{}'",
                    args[0], args[1]);
                Err(LispError::Type(s))
            }
        });

        let prim_pair = Primitive("pair".to_string(), |args| {
            num_args("pair", 2, args)?;
            Ok(Value::Pair(Box::new((args[0].clone(), args[1].clone()))))
        });


        let prim_list = Primitive("list".to_string(), |args| {
            fn prim_list(args: &[&Value]) -> Value {
                match args {
                    [] => Value::Nil,
                    [car, cdr @ ..] => Value::Pair(Box::new(((*car).clone(), prim_list(cdr)))),
                }
            }
            Ok(prim_list(args))
        });

        let prim_car = Primitive("car".to_string(), |args| {
            num_args("car", 1, args)?;
            if let Value::Pair(p) = args[0] {
                return Ok(p.0.clone());
            }

            let s = format!("'car' primitive expects a pair as argument, found: '{}'", args[0]);
            Err(LispError::Type(s))
        });

        let prim_cdr = Primitive("cdr".to_string(), |args| {
            num_args("car", 1, args)?;
            if let Value::Pair(p) = args[0] {
                return Ok(p.1.clone());
            }

            let s = format!("'cdr' primitive expects a pair as argument, found: '{}'", args[0]);
            Err(LispError::Type(s))
        });

        let prim_atomp = Primitive("atom?".to_string(), |args| {
            num_args("atom?", 1, args)?;
            if let Value::Pair(_) = args[0] {
                Ok(Value::Bool(false))
            } else {
                Ok(Value::Bool(true))
            }
        });

        let prim_symp = Primitive("sym?".to_string(), |args| {
            num_args("sym?", 1, args)?;
            if let Value::Symbol(_) = args[0] {
                Ok(Value::Bool(true))
            } else {
                Ok(Value::Bool(false))
            }
        });

        let prim_getchar = Primitive("getchar".to_string(), |args| {
            num_args("getchar", 0, args)?;
            let mut handle = io::stdin().lock();
            let mut buf = [0u8; 1];
            match handle.read(&mut buf) {
                Ok(0) => Ok(Value::Fixnum(-1)), // eof
                Ok(_) => Ok(Value::Fixnum(buf[0] as i64)),
                Err(e) => Err(LispError::Prim(format!("io error in 'getchar': {}", e))),
            }
        });

        let prim_print = Primitive("print".to_string(), |args| {
            num_args("print", 1, args)?;
            print!("{}", &args[0].to_string());
            _ = io::stdout().flush();
            Ok(Value::Symbol("ok".to_string()))
        });

        let prim_itoc = Primitive("itoc".to_string(), |args| {
            num_args("itoc", 1, args)?;
            if let Value::Fixnum(i) = args[0] {
                let c = match char::from_u32(*i as u32) {
                    Some(x) => x,
                    None => {
                        let s = format!("could not format integer '{}' as char", i);
                        return Err(LispError::Prim(s));
                    },
                };
                Ok(Value::Symbol(c.to_string()))

            } else {
                let s = format!("'itoc' primitive expects one integer argument, found: '{}'", args[0]);
                Err(LispError::Type(s))
            }
        });

        let prim_cat = Primitive("cat".to_string(), |args| {
            num_args("cat", 2, args)?;
            if let (Value::Symbol(a), Value::Symbol(b)) = (args[0], args[1]) {
                Ok(Value::Symbol(format!("{}{}", a, b)))
            } else {
                let s = format!("'cat' primitive expects two symbol arguments, found: '{}' and '{}'",
                    args[0], args[1]);
                Err(LispError::Type(s))
            }
        });

        let prim_div = Primitive("/".to_string(), |args| {
            num_args("/", 2, args)?;
            if let (Value::Fixnum(a), Value::Fixnum(b)) = (args[0], args[1]) {
                Ok(Value::Fixnum(a / b))
            } else {
                let s = format!("'cat' primitive expects two integer arguments, found: '{}' and '{}'",
                    args[0], args[1]);
                Err(LispError::Type(s))
            }
        });

        let prim_add = bin_fixnum_prim!("+", Value::Fixnum, +);
        let prim_sub = bin_fixnum_prim!("-", Value::Fixnum, -);
        let prim_mult = bin_fixnum_prim!("*", Value::Fixnum, *);
        let prim_le = bin_fixnum_prim!("<", Value::Bool, <);
        let prim_gt = bin_fixnum_prim!(">", Value::Bool, >);
        let prim_lte = bin_fixnum_prim!("<=", Value::Bool, <=);
        let prim_gte = bin_fixnum_prim!(">=", Value::Bool, >=);
        let prim_int_eq = bin_fixnum_prim!("=", Value::Bool, ==);

        let env = Env::new();
        let env = env.bind("+".to_string(), prim_add);
        let env = env.bind("eq".to_string(), prim_eq);
        let env = env.bind("-".to_string(), prim_sub);
        let env = env.bind("*".to_string(), prim_mult);
        let env = env.bind("/".to_string(), prim_div);
        let env = env.bind("<".to_string(), prim_le);
        let env = env.bind(">".to_string(), prim_gt);
        let env = env.bind("<=".to_string(), prim_lte);
        let env = env.bind(">=".to_string(), prim_gte);
        let env = env.bind("=".to_string(), prim_int_eq);
        let env = env.bind("mod".to_string(), prim_mod);
        let env = env.bind("pair".to_string(), prim_pair);
        let env = env.bind("list".to_string(), prim_list);
        let env = env.bind("car".to_string(), prim_car);
        let env = env.bind("cdr".to_string(), prim_cdr);
        let env = env.bind("atom?".to_string(), prim_atomp);
        let env = env.bind("sym?".to_string(), prim_symp);
        let env = env.bind("getchar".to_string(), prim_getchar);
        let env = env.bind("print".to_string(), prim_print);
        let env = env.bind("itoc".to_string(), prim_itoc);
        let env = env.bind("cat".to_string(), prim_cat);
        env
    }

    fn to_lo(&self) -> Value {
        let los: Vec<Value> = self.items
            .iter()
            .map(|(n, v)|
                Value::Pair(Box::new((Value::Symbol(n.clone()), match v.borrow().as_ref() {
                    Some(lo) => lo.clone(),
                    None => Value::Symbol("#<unspecified value>".to_string()),
                })))
            ).collect();
        Value::list_to_pair(los)
    }

    pub fn with_stdlib() -> Result<Env, StdlibError> {
        let path = std::path::Path::new("stdlib.lsp");
        let mut buf = String::new();

        let mut file = std::fs::File::open(path).map_err(|e| StdlibError::Io(e))?;
        let _ = file.read_to_string(&mut buf).map_err(|e| StdlibError::Io(e))?;

        let mut s = Stream::from_str(buf.as_str());

        let mut env = Env::basis();
        loop {
            let v = match s.read_value() {
                Ok(v) => v,
                Err(ParseError::Eof) => break,
                Err(ParseError::Lisp(e)) => return Err(StdlibError::Lisp(e)),
            };
            let ast = v.build_ast().map_err(|e| StdlibError::Lisp(e))?;

            env = ast.eval(env).map_err(|e| StdlibError::Lisp(e))?.1;
        }

        Ok(env)
    }
}

impl fmt::Display for Env {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let p = self.to_lo();
        write!(f, "{p}")
    }
}

/// Left-object
#[derive(Debug, PartialEq, Clone)]
pub enum Value {
    Fixnum(i64),
    Bool(bool),
    Symbol(String),
    Nil,
    Pair(Box<(Value, Value)>),
    Primitive(String, fn(&[&Value]) -> Result<Value, LispError>),
    Quote(Box<Value>),
    Closure(Vec<String>, Box<Expr>, Env),
}

fn reverse_list(mut xs: Value) -> Result<Value, String> {
    let mut out = Value::Nil;
    loop {
        match xs {
            Value::Nil => return Ok(out),
            Value::Pair(cell) => {
                let (car, cdr) = *cell;
                out = Value::Pair(Box::new((car, out)));
                xs = cdr;
            },
            _ => return Err("malformed list".to_string()),
        }
    }
}

/// Most efficient when T is a reference, as clone might otherwise perform unecessary allocations.
fn assert_unique<T: PartialEq + fmt::Display + Clone>(d: &dyn fmt::Display, xs: &[T]) -> Result<(), LispError> {
    if xs.len() <= 1 {
        return Ok(());
    }

    let x = xs[0].clone();
    let xs = &xs[1..];
    if xs.contains(&x) {
        let e = format!("'{}' expects unique bindings, found multiple of '{}'",
            d, x);
        return Err(LispError::Parse(e));
    }

    Ok(())
}

impl Value {
    fn is_list(&self) -> bool {
        match self {
            Value::Nil => true,
            Value::Pair(cell) => cell.1.is_list(),
            _ => false,
        }
    }

    pub fn build_ast(&self) -> Result<Expr, LispError> {
        use Value::*;
        match self {
            Primitive(_, _) | Closure(_, _, _) => unreachable!(), // shouldn't happen at this stage.
            Symbol(s) => Ok(Expr::Var(s.clone())),
            Pair(_) if self.is_list() => {
                match self.pair_to_list().as_slice() {
                    [] => Err(LispError::Parse("poorly formed expression".to_string())),
                    [sym, cond, if_true, if_false] if matches!(sym, Symbol(s) if s == "if") =>
                        Ok(Expr::If(Box::new((cond.build_ast()?, if_true.build_ast()?, if_false.build_ast()?)))),
                    [sym, c1, c2] if matches!(sym, Symbol(s) if s == "and") =>
                        Ok(Expr::And(Box::new((c1.build_ast()?, c2.build_ast()?)))),
                    [sym, c1, c2] if matches!(sym, Symbol(s) if s == "or") =>
                        Ok(Expr::Or(Box::new((c1.build_ast()?, c2.build_ast()?)))),
                    [sym, func, args] if matches!(sym, Symbol(s) if s == "apply") =>
                        Ok(Expr::Apply(Box::new((func.build_ast()?, args.build_ast()?)))),
                    [sym, Symbol(n), e] if matches!(sym, Symbol(s) if s == "val") =>
                        Ok(Expr::Def(Box::new(Definition::Val(n.clone(), e.build_ast()?)))),
                    [sym, e] if matches!(sym, Symbol(s) if s == "quote") =>
                        Ok(Expr::Literal((*e).clone())),
                    [sym, conditions @ ..] if matches!(sym, Symbol(s) if s == "cond") => {
                        fn cond_to_if(xs: &[&Value]) -> Result<Expr, LispError> {
                            if xs.is_empty() {
                                let s = "'cond' expects a list of conditions, found nothing".to_string();
                                return Err(LispError::Parse(s));
                            }

                            if let Value::Pair(b) = xs[0] {
                                let (cond, cell) = b.as_ref();
                                if let Value::Pair(b) = cell {
                                    if b.1 == Value::Nil {
                                        let rest = b.0.clone();
                                        let cond_ast = cond.build_ast()?;
                                        let rest_ast = rest.build_ast()?;
                                        let conds = if xs.len() == 1 {
                                            Expr::Literal(Value::Symbol("error".to_string()))
                                        } else {
                                            cond_to_if(&xs[1..])?
                                        };
                                        let b = Box::new((cond_ast, rest_ast, conds));
                                        return Ok(Expr::If(b));
                                    }
                                }
                            }

                            let s = format!("'cond' expects a list of conditions, found '{}'", xs[0]);
                            Err(LispError::Parse(s))
                        }

                        cond_to_if(conditions)
                    },
                    [sym, ns, e] if ns.is_list() && matches!(sym, Symbol(s) if s == "lambda") => {
                        let formals = ns
                            .pair_to_list()
                            .into_iter()
                            .map(|l| match l {
                                Symbol(s) => Ok(s.clone()),
                                _ => {
                                    let s = format!("arguments to lambda can only be symbols, found: '{}'", l);
                                    Err(LispError::Type(s))
                                },
                            })
                            .collect::<Result<Vec<String>, LispError>>()?;
                        assert_unique(&"lambda", formals.as_slice())?;
                        let ast = e.build_ast()?;
                        Ok(Expr::Lambda(formals, Box::new(ast)))
                    },
                    [sym, Symbol(n), ns, e] if matches!(sym, Symbol(s) if s == "define") => {
                        let formals = ns
                            .pair_to_list()
                            .into_iter()
                            .map(|l| match l {
                                Symbol(s) => Ok(s.clone()),
                                _ => {
                                    let s = format!("arguments to lambda can only be symbols, found: '{}'", l);
                                    Err(LispError::Type(s))
                                },
                            })
                            .collect::<Result<Vec<String>, LispError>>()?;
                        assert_unique(&"define", formals.as_slice())?;
                        let ast = e.build_ast()?;
                        Ok(Expr::Def(Box::new(Definition::Fun(n.clone(), formals, ast))))
                    },
                    [Symbol(s), bindings, exp] if bindings.is_list() && Lets::is_valid(s) => {
                        let l = Lets::map(s).unwrap();

                        fn make_binding(b: &Value) -> Result<(String, Expr), LispError> {
                            if let Pair(b) = b {
                                if let (Symbol(n), Pair(b)) = b.as_ref() {
                                    if let (e, Nil) = b.as_ref() {
                                        return Ok((n.clone(), e.build_ast()?));
                                    }
                                }
                            }

                            let s = format!("binding expects '(name as)', found: '{}'", b);
                            Err(LispError::Parse(s))
                        }

                        let bindings = bindings
                            .pair_to_list()
                            .iter()
                            .map(|b| make_binding(*b))
                            .collect::<Result<Vec<(String, Expr)>, LispError>>()?;

                        // let* enables imperative-style lets such as
                        // (let* ((x 5)
                        //     (x (factorial x))
                        //     (x (sqrt x))
                        //     (x (to-string x)))
                        //   (print x))
                        if l != Lets::Star {
                            let names: Vec<&str> = bindings
                                .iter()
                                .map(|x| x.0.as_str())
                                .collect();
                            assert_unique(&l, names.as_slice())?;
                        }

                        let bindings = bindings
                            .iter()
                            .map(|x| (x.to_owned().0, Box::new(x.to_owned().1)))
                            .collect::<Vec<(String, Box<Expr>)>>();
                        Ok(Expr::Let(l, bindings, Box::new(exp.build_ast()?)))
                    },
                    [func, args @ ..] => {
                        let mut values = Vec::with_capacity(args.len());
                        for arg in args {
                            values.push(arg.build_ast()?);
                        }
                        Ok(Expr::Call(Box::new((func.build_ast()?, values))))
                    },
                }
            },
            Fixnum(_) | Bool(_) | Nil | Pair(_) | Quote(_) => Ok(Expr::Literal(self.clone())),
        }
    }

    fn pair_to_list(&self) -> Vec<&Value> {
        let mut out = Vec::new();
        let mut p = self;
        loop {
            match p {
                Value::Pair(cell) => {
                    let (fst, snd) = cell.as_ref();
                    out.push(fst);
                    p = snd;
                },
                Value::Nil => break,
                _ => panic!("malformed list"),
            }
        }
        out
    }

    fn list_to_pair(xs: Vec<Value>) -> Value {
        let mut acc = Value::Nil;
        for v in xs.into_iter().rev() {
            acc = Value::Pair(Box::new((v, acc)));
        }
        acc
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    Literal(Value),
    Var(String),
    If(Box<(Expr, Expr, Expr)>),
    And(Box<(Expr, Expr)>),
    Or(Box<(Expr, Expr)>),
    Apply(Box<(Expr, Expr)>),
    Call(Box<(Expr, Vec<Expr>)>),
    Lambda(Vec<String>, Box<Expr>),
    /// (kind, bindings, in)
    Let(Lets, Vec<(String, Box<Expr>)>, Box<Expr>),
    Def(Box<Definition>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Lets {
    Fixed, Star, Rec,
}

impl Lets {
    fn is_valid(s: &str) -> bool {
        Lets::map(s).is_some()
    }

    fn map(s: &str) -> Option<Lets> {
        use Lets::*;
        match s {
            "let" => Some(Fixed),
            "let*" => Some(Star),
            "letrec" => Some(Rec),
            _ => None,
        }
    }
}

impl fmt::Display for Lets {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Lets::*;
        match self {
            Fixed => write!(f, "let"),
            Star => write!(f, "let*"),
            Rec => write!(f, "letrec"),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Definition {
    Val(String, Expr),
    Fun(String, Vec<String>, Expr),
}

fn eval_bindings(
    bindings: &Vec<(String, Box<Expr>)>,
    env: &Env
) -> Result<Vec<(String, Value)>, LispError> {
    bindings
        .iter()
        .map(|(name, expr)| {
            let value = expr.eval_expr(env)?;
            Ok((name.clone(), value))
        })
        .collect()
}

impl Expr {
    pub fn eval(&self, env: Env) -> Result<(Value, Env), LispError> {
        match self {
            Expr::Def(d) => Expr::eval_def(d, env),
            e => Ok((e.eval_expr(&env)?, env)),
        }
    }

    /// Returns the modified env.
    fn eval_def(def: &Definition, env: Env) -> Result<(Value, Env), LispError> {
        match def {
            Definition::Val(n, e) => {
                let v = e.eval_expr(&env)?;
                let env_prime = env.bind(n.clone(), v.clone());
                Ok((v, env_prime))
            },
            Definition::Fun(n, ns, e) => {
                let lambda = Expr::Lambda(ns.clone(), Box::new(e.clone()));
                let (formals, body, cl_env) = match lambda.eval_expr(&env)? {
                    Value::Closure(fs, body, env) => (fs, body, env),
                    v => {
                        let s = format!("expected a closure to define a function, found: '{}'", v);
                        return Err(LispError::Type(s));
                    }
                };
                let loc = Env::make_loc();
                let clo = Value::Closure(formals, body, cl_env.bind_rc(n.to_string(), loc.clone()));
                *loc.borrow_mut() = Some(clo.clone());
                Ok((clo, env.bind_rc(n.to_string(), loc)))
            },
        }
    }

    /// Does not modify env, and returns the evaluated expression.
    fn eval_expr(&self, env: &Env) -> Result<Value, LispError> {
        use Expr::*;

        match self {
            Def(_) => unreachable!(),
            Literal(Value::Quote(b)) => Ok(*b.clone()),
            Literal(l) => Ok(l.clone()),
            Var(n) => env.lookup(&n),
            If(b) => match (*b).0.eval_expr(env)? {
                Value::Bool(true) => Ok((*b).1.eval_expr(env)?),
                Value::Bool(false) => Ok((*b).2.eval_expr(env)?),
                other => {
                    let s = format!("if statement condition did not resolve to a bool, found: '{}'", other);
                    Err(LispError::Type(s))
                },
            },
            And(b) => match ((*b).0.eval_expr(env)?, (*b).1.eval_expr(env)?) {
                (Value::Bool(v1), Value::Bool(v2)) => Ok(Value::Bool(v1 && v2)),
                (v1, v2) => {
                    let s = format!("and statement conditions did not resolve to bools, found: '{}' and '{}'",
                        v1, v2);
                    Err(LispError::Type(s))
                },
            },
            Or(b) => match ((*b).0.eval_expr(env)?, (*b).1.eval_expr(env)?) {
                (Value::Bool(v1), Value::Bool(v2)) => Ok(Value::Bool(v1 || v2)),
                (v1, v2) => {
                    let s = format!("or statement conditions did not resolve to bools, found: '{}' and '{}'",
                        v1, v2);
                    Err(LispError::Type(s))
                },
            },
            Apply(b) => {
                let f = (*b).0.eval_expr(env)?;
                let arg_list = (*b).1.eval_expr(env)?;
                if !arg_list.is_list() {
                    let s = format!("cannot apply a non-list '{}' to a primitive", arg_list);
                    return Err(LispError::Type(s));
                }

                let mut primed = Vec::new();
                for arg in arg_list.pair_to_list() {
                    let ast = arg.build_ast()?;
                    primed.push(ast.eval_expr(env)?);
                }
                Expr::eval_apply(f, primed)
            },
            Call(b) => {
                if let (Expr::Var(name), true) = (&(*b).0, (*b).1.is_empty()) {
                    if name == "env" {
                        let vs: Vec<Value> = env.items
                            .iter()
                            .map(|(n, v)|
                                Value::Pair(Box::new((Value::Symbol(n.clone()), match v.borrow().as_ref() {
                                    Some(v) => v.clone(),
                                    None => Value::Symbol("#<unspecified value>".to_string()),
                                })))
                            ).collect();
                        let env = Value::list_to_pair(vs);
                        return Ok(env.clone());
                    }
                }

                let f = (*b).0.eval_expr(env)?;

                let args = &(*b).1;
                let mut primed = Vec::with_capacity(args.len());
                for arg in args.iter() {
                    primed.push(arg.eval_expr(env)?);
                }

                Expr::eval_apply(f, primed)
            },
            Lambda(ns, e) => Ok(Value::Closure(ns.clone(), e.clone(), env.clone())),
            Let(Lets::Fixed, bindings, body) => {
                let mapped = eval_bindings(bindings, env)?;
                body.eval_expr(&env.clone().bind_list(mapped))
            },
            Let(Lets::Star, bindings, body) => {
                let mut bound_env = env.clone();
                for (name, b) in bindings.iter() {
                    let v = b.eval_expr(&bound_env)?;
                    bound_env = bound_env.bind(name.clone(), v);
                }
                body.eval_expr(&bound_env)
            },
            Let(Lets::Rec, bindings, body) => {
                let mut env_prime = env.clone();
                for (name, _) in bindings.iter() {
                    let empty = Env::make_loc();
                    env_prime = env_prime.bind_rc(name.clone(), empty);
                }
                let updated = eval_bindings(bindings, &env_prime)?;
                for (name, value) in updated.into_iter() {
                    let mut mutted = env_prime.lookup_mut(name.as_str())?;
                    *mutted = Some(value);
                }
                body.eval_expr(&env_prime)
            },
        }
    }

    fn eval_apply(f: Value, values: Vec<Value>) -> Result<Value, LispError> {
        match f {
            Value::Primitive(_, f) => f(values.iter().collect::<Vec<&Value>>().as_slice()),
            Value::Closure(ns, e, cl_env) => {
                let zipped: Vec<(String, Value)> = ns
                    .into_iter()
                    .zip(values)
                    .collect();
                e.eval_expr(&cl_env.bind_list(zipped))
            },
            _ => {
                let s = format!("tried to call a non-function, found: '{}'", f);
                Err(LispError::Type(s))
            },
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Value::*;

        match self {
            Fixnum(x) => write!(f, "{x}"),
            Bool(b) => write!(f, "{}", if *b { "#t" } else { "#f" }),
            Symbol(s) => write!(f, "{s}"),
            Nil => write!(f, "nil"),
            Pair(b) => {
                write!(f, "(")?;
                if self.is_list() {
                    let mut p = b;
                    loop {
                        write!(f, "{}", p.0)?;
                        match &p.1 {
                            Pair(np) => {
                                p = &np;
                            },
                            Nil => break,
                            _ => panic!("malformed list"),
                        }
                        write!(f, " ")?;
                    }
                } else {
                    write!(f, "{} . {}", b.0, b.1)?;
                }
                write!(f, ")")
            },
            Primitive(name, _) => write!(f, "#<primitive:{name}>"),
            Quote(q) => write!(f, "'{}", *q),
            Closure(_, _, _) => write!(f, "#<closure>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<Value, ParseError> {
        let mut s = Stream::from_str(input);
        s.read_value()
    }

    fn eval_result(env: Env, input: &str) -> Result<(Value, Env), LispError> {
        let e = parse(input).unwrap();
        let ast = e.build_ast()?;
        ast.eval(env)
    }

    fn eval(env: Env, input: &str) -> (Value, Env) {
        eval_result(env, input).unwrap()
    }

    #[test]
    fn read_char() {
        let mut s = Stream::from_str("hello  \n\t world");

        assert_eq!(s.read_char().unwrap(), 'h');
        assert_eq!(s.read_char().unwrap(), 'e');
        assert_eq!(s.read_char().unwrap(), 'l');
        assert_eq!(s.read_char().unwrap(), 'l');
        assert_eq!(s.read_char().unwrap(), 'o');
        assert_eq!(s.read_char().unwrap(), ' ');
        assert_eq!(s.read_char().unwrap(), ' ');

        assert_eq!(s.line_num, 1);
        assert_eq!(s.read_char().unwrap(), '\n');
        assert_eq!(s.line_num, 2);

        assert_eq!(s.read_char().unwrap(), '\t');
        assert_eq!(s.line_num, 2);

        assert_eq!(s.read_char().unwrap(), ' ');
        assert_eq!(s.read_char().unwrap(), 'w');
        assert_eq!(s.read_char().unwrap(), 'o');
        assert_eq!(s.read_char().unwrap(), 'r');
        assert_eq!(s.read_char().unwrap(), 'l');
        assert_eq!(s.read_char().unwrap(), 'd');
        assert_eq!(s.read_char(), None); // eof
    }

    #[test]
    fn test_pair_to_list() {
        let mut s = Stream::from_str("(1 2 3 4 5 350)");

        let value = s.read_value().unwrap();
        let vec = value.pair_to_list();
        let exp: Vec<Value> = vec![1 as i64, 2, 3, 4, 5, 350].iter().map(|x| Value::Fixnum(*x)).collect();
        assert_eq!(vec.len(), exp.len());

        for (i, val) in vec.iter().enumerate() {
            assert_eq!(**val, exp[i]);
        }
    }

    #[test]
    fn unread_char() {
        let mut s = Stream::from_str("ab\nc");

        assert_eq!(s.read_char().unwrap(), 'a');
        s.unread_char('a');
        assert_eq!(s.peek_char().unwrap(), 'a');
        assert_eq!(s.read_char().unwrap(), 'a');
        assert_eq!(s.read_char().unwrap(), 'b');

        assert_eq!(s.line_num, 1);
        assert_eq!(s.read_char().unwrap(), '\n');
        assert_eq!(s.line_num, 2);
        s.unread_char('\n');
        assert_eq!(s.line_num, 1);
        assert_eq!(s.peek_char().unwrap(), '\n');
        assert_eq!(s.read_char().unwrap(), '\n');
        assert_eq!(s.line_num, 2);

        assert_eq!(s.read_char().unwrap(), 'c');
        assert_eq!(s.read_char(), None);
    }

    #[test]
    fn parse_simples() {
        let mut s = Stream::from_str("   12   \n15 340 #t #f ~90 hello_world blisp");

        assert_eq!(s.read_value().unwrap(), Value::Fixnum(12));
        assert_eq!(s.line_num, 1);
        assert_eq!(s.read_value().unwrap(), Value::Fixnum(15));
        assert_eq!(s.line_num, 2);
        assert_eq!(s.read_value().unwrap(), Value::Fixnum(340));
        assert_eq!(s.read_value().unwrap(), Value::Bool(true));
        assert_eq!(s.read_value().unwrap(), Value::Bool(false));
        assert_eq!(s.read_value().unwrap(), Value::Fixnum(-90));
        assert_eq!(s.read_value().unwrap(), Value::Symbol("hello_world".to_string()));
        assert_eq!(s.read_value().unwrap(), Value::Symbol("blisp".to_string()));

        let mut s = Stream::from_str("(1 2 hello world) (34 (35 some))");
        assert_eq!(s.read_value().unwrap().to_string(), "(1 2 hello world)");
        assert_eq!(s.read_value().unwrap().to_string(), "(34 (35 some))");
    }

    #[test]
    fn parse_errors() {
        assert!(
            matches!(parse("("), Err(ParseError::Lisp(LispError::Parse(e))) if e.contains("unexpected eof in list"))
        );
        assert!(
            matches!(parse(")"), Err(ParseError::Lisp(LispError::Parse(e))) if e.contains("unexpected char"))
        );
    }

    #[test]
    fn eval_forms() {
        assert_eq!(eval(Env::new(), "(if #t (if #t 1 2) 3)").0.to_string(), "1");
        assert_eq!(
            eval(Env::basis(), "(if #f (if #t 1 2) (if #t (list 34 35) 12))").0.to_string(),
            "(34 35)"
        );

        let env = Env::new();
        let (res, env) = eval(env, "(val x #t)");
        assert_eq!(res.to_string(), "#t");

        let (res, env) = eval(env, "(val y (if x ~12 13))");
        assert_eq!(res.to_string(), "-12");

        assert_eq!(env.lookup("x").unwrap(), Value::Bool(true));
        assert_eq!(env.lookup("y").unwrap(), Value::Fixnum(-12));
    }

    #[test]
    fn eval_basis() {
        assert_eq!(eval(Env::basis(), "(+ 12 13)").0.to_string(), "25");
        assert_eq!(eval(Env::basis(), "(pair 12 13)").0.to_string(), "(12 . 13)");
        assert_eq!(eval(Env::basis(), "(pair (pair 12 13) 14)").0.to_string(), "((12 . 13) . 14)");
        assert_eq!(eval(Env::basis(), "(pair 12 (pair 13 14))").0.to_string(), "(12 . (13 . 14))");
        assert_eq!(eval(Env::basis(), "(eq ((lambda (x) (+ x 1)) 10) 11)").0.to_string(), "#t");
        assert_eq!(eval(Env::basis(), "(eq ((lambda (x) (+ x 1)) 10) 12)").0.to_string(), "#f");
        assert_eq!(eval(Env::basis(), "(itoc 128175)").0.to_string().as_str(), "💯");
    }

    #[test]
    fn eval_env_form() {
        let env = Env::basis();
        let (result, env_prime) = eval(env.clone(), "(env)");
        assert_eq!(result.to_string(), env.to_string());
        assert_eq!(env_prime.to_string(), env.to_string());
    }

    #[test]
    fn eval_error_paths() {
        assert!(
            matches!(
                eval_result(Env::basis(), "(apply + 1)"),
                Err(LispError::Type(e)) if e.contains("cannot apply a non-list")
            )
        );
        assert!(
            matches!(
                eval_result(Env::basis(), "(1 2)"),
                Err(LispError::Type(e)) if e.contains("tried to call a non-function")
            )
        );
        assert!(
            matches!(
                eval_result(Env::basis(), "(if 1 2 3)"),
                Err(LispError::Type(e)) if e.contains("if statement condition did not resolve to a bool")
            )
        );
        assert!(
            matches!(
                eval_result(Env::basis(), "(and #t 1)"),
                Err(LispError::Type(e)) if e.contains("and statement conditions did not resolve to bools")
            )
        );
    }

    #[test]
    fn eval_applications_and_quotes() {
        let (result, _) = eval(Env::basis(), "(apply + (list 13 14))");
        assert_eq!(result, Value::Fixnum(27));

        let (result, _) = eval(Env::basis(), "(apply + '((if #t ~12 13) 14))");
        assert_eq!(result, Value::Fixnum(2));

        let (q1, _) = eval(Env::basis(), "'(if #t 1 2)");
        let (q2, _) = eval(Env::basis(), "(quote (if #t 1 2))");
        assert_eq!(q1, q2);
        assert_eq!(q1.to_string(), "(if #t 1 2)");
    }

    #[test]
    fn eval_lambda() {
        let env = Env::basis();
        let (_, env) = eval(env, "(val add-one (lambda (x) (+ x 1)))");
        let (res, env) = eval(env, "(add-one 12)");
        assert_eq!(res, Value::Fixnum(13));

        let (_, env) = eval(env, "(val add-three (lambda (x) (add-one (add-one (add-one x)))))");
        let (res, _) = eval(env, "(add-three ~90)");
        assert_eq!(res, Value::Fixnum(-87));
    }

    #[test]
    fn define_and_eval_function() {
        let env = Env::basis();
        let (_, env) = eval(env, "(define f (x) (if (eq x 0) 1 (* x (f (+ x ~1)))))");
        let (res, env) = eval(env, "(f 4)");
        assert_eq!(res, Value::Fixnum(24));

        let (res, env) = eval(env, "(f 5)");
        assert_eq!(res, Value::Fixnum(120));

        let (res, _) = eval(env, "(f 6)");
        assert_eq!(res, Value::Fixnum(720));
    }

    #[test]
    fn eval_cond() {
        let env = Env::basis();
        let cond = "(cond ((< x 4) 'lower)
                          ((= x 4) 'equal)
                          ((> x 4) 'higher))";
        let (_, env) = eval(env, "(val x 3)");
        let (res, env) = eval(env, cond);
        assert_eq!(res.to_string(), "lower".to_string());

        let (_, env) = eval(env, "(val x 4)");
        let (res, env) = eval(env, cond);
        assert_eq!(res.to_string(), "equal".to_string());

        let (_, env) = eval(env, "(val x 5)");
        let (res, _) = eval(env, cond);
        assert_eq!(res.to_string(), "higher".to_string());

    }

    #[test]
    fn let_regular() {
        let env = Env::basis();
        let (_, env) = eval(env, "(define f (x) (cond ((< x 4) 'lower)
                                                      ((= x 4) 'equal)
                                                      ((> x 4) 'higher)))");

        let (res, env) = eval(env, "(let ((x 5)) (f x))");
        assert_eq!(res.to_string(), "higher");

        let (res, env) = eval(env, "(let ((z 4)) (f z))");
        assert_eq!(res.to_string(), "equal");

        let (res, env) = eval(env, "(let ((value 3)) (f value))");
        assert_eq!(res.to_string(), "lower");

        let (_, env) = eval(env, "(val x 34)");
        let (res, _env) = eval(env, "(let ((a (+ x 32))) (+ a 1))");
        assert_eq!(res, Value::Fixnum(67));
    }

    #[test]
    fn let_star() {
        let env = Env::basis();
        let (res, env) = eval(env, "(let* ((x 34) (y x)) y)");
        assert_eq!(res, Value::Fixnum(34));

        let (res, _env) = eval(env, "(let* ((x 34) (y (+ x 33))) y))");
        assert_eq!(res, Value::Fixnum(67));
    }

    #[test]
    fn let_shadow() {
        let env = Env::basis();
        let (_, env) = eval(env, "(val x 5)");
        let (res, env) = eval(env, "(let* ((x 4) (x (* x 4))) x)");
        assert_eq!(res, Value::Fixnum(16));

        let (_, env) = eval(env, "(val y 20)");
        // let ==> y should depend on (val x 5)
        let (res, env) = eval(env, "(let ((x 4) (y (* x x))) (+ x y))");
        assert_eq!(res, Value::Fixnum(29));

        // let* ==> y should depend on the previous x
        let (res, _env) = eval(env, "(let* ((x 4) (y (* x x))) (+ x y))");
        assert_eq!(res, Value::Fixnum(20));
    }

    #[test]
    fn unique_binding_errors() {
        assert!(
            matches!(
                eval_result(Env::basis(), "(let ((x 1) (x 2)) x)"),
                Err(LispError::Parse(e)) if e.contains("'let' expects unique bindings")
            )
        );
        assert!(
            matches!(
                eval_result(Env::basis(), "(val f (lambda (x x) x))"),
                Err(LispError::Parse(e)) if e.contains("'lambda' expects unique bindings")
            )
        );
    }

    #[test]
    fn let_rec() {
        let env = Env::basis();
        let (res, env) = eval(env, "(letrec ((f (lambda (x) (g (+ x 1))))
                                              (g (lambda (x) (+ x 3))))
                                       (f 0))");
        assert_eq!(res, Value::Fixnum(4));

        let (res, _env) = eval(env, "(letrec ((factorial (lambda (x) (
                                                            if (< x 2)
                                                              1
                                                              (* x (factorial (- x 1)))))))
                                             (factorial 5))");
        assert_eq!(res, Value::Fixnum(120));
    }

    #[test]
    fn composition_function() {
        let env = Env::basis();
        let (_, env) = eval(env, "(define o (f g) (lambda (x) (f (g x))))");
        let (_, env) = eval(env, "(define f (x) (+ x 1))");

        let (res, env) = eval(env, "((o f f) 4)");
        assert_eq!(res, Value::Fixnum(6));

        let (_, env) = eval(env, "(define factorial (x) (if (< x 2) 1 (* x (factorial (- x 1)))))");
        let (res, _env) = eval(env, "((o factorial f) 4)");
        assert_eq!(res, Value::Fixnum(120));
    }

    #[test]
    fn okay_stdlib() {
        // will panic on faulty std.
        let _ = Env::with_stdlib().unwrap();
    }
}
