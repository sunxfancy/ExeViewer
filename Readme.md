ExeViewer
=========

ExeViewer is evolving into a Linux ELF cross-environment compatibility diagnostic
tool for developers: explain why an executable or shared library built in one
environment fails in another, with evidence and actionable fixes.

See the [product design](docs/product-design.md) for scope, diagnostic principles,
CLI design, and implementation stages. Today the project provides an interactive
ELF viewer and a non-interactive static report; full compatibility diagnosis is
planned, not yet implemented.

## Features

- View the section headers of the executable file.
- Disassemble the text section of the executable file.
- View Dynamic Symbols and PLT table
- Print static ELF metadata and declared dependencies without a terminal.
- Currently only supports ELF files and x86_64 architecture.
  
![1.png](./docs/1.png)
![2.png](./docs/2.png)
![3.png](./docs/3.png)
![4.png](./docs/4.png)
![5.png](./docs/5.png)

## Installation

You can install the tool by running the following command (you need to have `cargo` and `rust` installed on your system):

```bash
cargo install --git https://github.com/sunxfancy/ExeViewer
```


## Build

To build the project, you need to have `rust` installed on your system. You can install it from [here](https://www.rust-lang.org/tools/install).

After installing `rust`, you can build the project by running the following command:

```bash
cargo build --release
```

This will create the executable in the `target/release` directory.

## Usage

To view the contents of an executable file, you can run the following command:

```bash
ExeViewer <path-to-executable>
```

To print information directly without interaction:

```bash
ExeViewer --report ./app
ExeViewer --report /path/to/libfoo.so
ExeViewer ./app > report.txt
```

Redirected stdout automatically selects report mode. Use `--interactive` to
explicitly select the TUI (requires a terminal). Report mode never runs the target
or its interpreter. It prints metadata, interpreter, and declared
NEEDED/RPATH/RUNPATH/SONAME entries, and explicitly marks compatibility as
**not assessed**. Recursive resolution and symbol-version diagnostics are planned.
Missing section metadata is reported as unknown rather than no dependencies.

Reports go to stdout and errors to stderr. Successful inspection exits with 0;
input or mode errors return nonzero. Text output is intended for people; a stable
JSON diagnostic format is planned. The Cargo binary name is `ExeViewer`.


## License

MIT License (c) 2024 sunxfancy
