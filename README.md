Blisp
=========

> An implementation of a small lisp interpreter written in Rust

The core interpreter largely follows the [Lisp article
series](https://bernsteinbear.com/blog/lisp/00_fundamentals/) by Max Bernstein.
It is quite a fun read, and is highly educational.

This projects aims to be small, memory efficient and readable, in that order of
performance.

## Why?

Because it's fun and I've been learning a lot. I've never really touched
functional programming, apart from some small-scale toy-projects in OCaml, so
implementing a small lisp interpreter from scratch has shown me a lot.

Lately, building software at a low level, i.e managing individual bytes and
heap allocations, has been a pleasure. I have experience with C/Zig-style
memory management (i.e alloc and free), but not with RAII. I chose to write
this interpreter in Rust to familiarize myself with modern RAII. Also the
article series I followed allowed me to largely focus on learning Rust and its
style of memory management, instead of fighting with developing the correct
logic.

## Installation

Only prerequisite to build blisp is Rust. All of the external dependencies
are installed via Cargo, therefore to build a binary, execute:

```console
cargo build --release
```

## Repl example
```console
➜  $ cargo run -q
$> ; Comments are denoted with ';'
$> ; Lets see the entire environment.
$> ; These are the builtin primitives, and the stdlib.
$> (env)
((+ . #<primitive:+>) (eq . #<primitive:eq>) (- . #<primitive:->) (* . #<primitive:*>) (/ . #<primitive:/>) (< . #<primitive:<>) (> . #<primitive:>>) (<= . #<primitive:<=>) (>= . #<primitive:>=>) (= . #<primitive:=>) (mod . #<primitive:mod>) (pair . #<primitive:pair>) (list . #<primitive:list>) (car . #<primitive:car>) (cdr . #<primitive:cdr>) (atom? . #<primitive:atom?>) (sym? . #<primitive:sym?>) (getchar . #<primitive:getchar>) (print . #<primitive:print>) (itoc . #<primitive:itoc>) (cat . #<primitive:cat>) (o . #<closure>) (caar . #<closure>) (cadr . #<closure>) (caddr . #<closure>) (cadar . #<closure>) (caddar . #<closure>) (cons . #<primitive:pair>) (newline .
) (space .  ) (getline . #<closure>) (null? . #<closure>) (length . #<closure>) (take . #<closure>) (drop . #<closure>) (merge . #<closure>) (mergesort . #<closure>) (map . #<closure>) (mem . #<closure>) (find . #<closure>) (filter . #<closure>) (range . #<closure>))
$>
$> ; Simple addition
$> (+ 1 2)
3
$>
$> ; Create a list from 0 to 10
$> (range 0 10)
(0 1 2 3 4 5 6 7 8 9)
$>
$> ; Add 2 to each element in the list from 0 to 10
$> (map (lambda (x) (+ x 2)) (range 0 10))
(2 3 4 5 6 7 8 9 10 11)
$>
$> ; Define the factorial function (note that the definition is recursive)
$> (define factorial (x) (if (< x 2) 1 (* x (factorial (- x 1)))))
#<closure>
$>
$> ; Check that the factorial function is correct
$> (map factorial (list 1 2 3 4 5))
(1 2 6 24 120)
$>
$> ; We even support let with lambdas.
$> (let ((is-even (lambda (x) (= (mod x 2) 0)))) (filter is-even (range 0 31)))
(0 2 4 6 8 10 12 14 16 18 20 22 24 26 28 30)
$>
```

## Implementation

Here are the key structures used in the interpreter:
- `Value`: Basic building blocks of the lisp, ex:
    - Booleans
    - Integers,
    - Nil values
    - Closures
- `Expr`: Contains data (commonly strings or more expressions) that define some
  syntax, ex:
    - Literals
    - If expressions
    - Function applications
    - Defines/Lets
- `Env`: Stores a set of strings mapped to a `Value`. We use this to store
  `val` and `define` expressions. In the repl, run `(env)` to see the
  environment.

The interpreter is divided into these main steps:
1. Lexical analysis: The `Stream` struct is responsible for reading the next
   `Value`.
2. Building AST: Once a root `Value` has been established, we call
   `value.build_ast()` which returns a root `Expression`, which is
   the parsed AST.
3. AST evaluation: We call `ast.eval(env)` which calls depending on the ast
   either `eval_def` or `eval_expr`, which either way returns a pair of the
   resulting value and a new environment. Note that only `eval_def` is allowed
   to modify (i.e return a modified version of `env`) because `eval_expr` is
   meant to only perform pure calculations, and not modify the state. Also note
   that this is easier to do in Rust, given that `env` is passed to `eval` as
   owned, and can therefore be easily modified and returned.

Currently, the **main** branch's lisp AST contains methods to self-evaluate,
meaning that there is no underlying backend or way to efficiently compile and
store a program.

We mean to rectify this, by compiling the AST into an IR, which a custom VM can
further optimize and run. This also allows the interpreter to store a program
as bytecode, which the VM could load and run without having to parse the lisp
again.

#### Roadmap

- [ ] Implement proper stack-frames in the VM
- [ ] Implement RAM/Heap storage in the VM
- [ ] Actually compile the AST to VM IR
- [ ] Tidy up the lisp's parsing and AST building allocations

### Goals, ideas & experiments

Virtual machine:
- JIT-compilation?
- Garbage collection for RAM/Heap
- Implement multithreading?
- Disassembler
