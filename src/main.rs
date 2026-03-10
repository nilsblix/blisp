#![allow(dead_code)]

use std::io::{Read, self};

mod lisp;
mod vm;

use lisp::{Env, Stream};

fn repl<S: Iterator<Item = char>>(
    stream: &mut Stream<S>,
    env: Env,
) -> io::Result<Env> {
    use lisp::ParseError::*;

    let mut e = env;
    loop {
        let expr = match stream.read_value() {
            Ok(v) => v,
            Err(Eof) => break,
            Err(Lisp(e)) => {
                println!("error: lisp: {e}");
                continue;
            }
        };

        let ast = match expr.build_ast() {
            Ok(a) => a,
            Err(l) => {
                println!("error: lisp: {l}");
                continue;
            },
        };

        let (result, env_prime) = match ast.eval(e.clone()) {
            Ok(x) => x,
            Err(l) => {
                println!("error: lisp: {l}");
                continue;
            },
        };
        e = env_prime;

        println!("{result}");
    }
    Ok(e)
}

// fn main() -> Result<(), Box<dyn std::error::Error>> {
//     let mut asm_file = std::fs::File::open("program.vasm")?;
//     let mut asm = String::new();
//     _ = asm_file.read_to_string(&mut asm);
//     let mut assembler = vm::asm::Assembler::new();
//     let program = match assembler.assemble_str(&asm) {
//         Ok(is) => is,
//         Err(e) => {
//             println!("{e}");
//             return Ok(());
//         },
//     };
//
//     let f = std::fs::File::create("program.vm")?;
//     vm::save_program(f, program.as_slice())?;
//
//     // TIME TO LOAD
//
//     const MEM_CAPACITY: usize = 16 * 1024;
//     let mut stack = [0u8; MEM_CAPACITY];
//     let mut program = [vm::Instruction::zeroed(); MEM_CAPACITY];
//
//     let f = std::fs::File::open("program.vm")?;
//     let mut m = vm::Machine::from_reader(f, &mut stack, &mut program)?;
//
//     m.run()?;
//     let res = m.last_value().unwrap();
//     println!("res: {res}");
//
//     Ok(())
// }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use rustyline::error::ReadlineError;
    use rustyline::DefaultEditor;

    let mut env = match Env::with_stdlib() {
        Ok(env) => env,
        Err(e) => {
            println!("{e}");
            return Ok(());
        },
    };

    let args: Vec<String> = std::env::args().collect();
    if let Some(path) = args.get(1) {
        let p = std::path::Path::new(path);
        let mut f = std::fs::File::open(p)?;

        let mut buf = Vec::new();
        let _ = f.read_to_end(&mut buf)?;

        let s = String::from_utf8(buf).unwrap_or_else(|_| panic!("bad..."));
        let mut stream = lisp::Stream::from_str(s.as_str());
        _ = repl(&mut stream, env)?;
        return Ok(());
    }

    let mut rl = DefaultEditor::new()?;
    loop {
        let readline = rl.readline("$> ");
        match readline {
            Ok(line) => {
                let mut stream = lisp::Stream::from_str(line.as_str());
                env = repl(&mut stream, env)?;
            },
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => break,
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
    Ok(())
}

