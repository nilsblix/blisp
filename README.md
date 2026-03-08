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
style of memory management, instead of fighting with the developing correct
logic.

## Installation

Only prerequisite to build blisp is Rust. All of the external dependencies
are installed via Cargo, therefore to build a binary, execute:

```console
cargo build --release
```

## Implementation

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
- [ ] Tidy up the lisp's parsing and AST building allocations

### Goals, ideas & experiments

Virtual machine:
- JIT-compilation?
- Garbage collection for RAM/Heap
- Implement multithreading?
- Disassembler
