ExeViewer
=========

ExeViewer is evolving into a Linux ELF cross-environment compatibility diagnostic
tool for developers: explain why an executable or shared library built in one
environment fails in another, with evidence and actionable fixes.

See the [product design](docs/product-design.md) for scope, diagnostic principles,
CLI design, and implementation stages. The project provides an interactive ELF viewer, non-interactive metadata reports,
static glibc startup compatibility diagnosis, A/B report comparison, and explicit
loader tracing. See the [CLI guide](docs/cli.md) for contracts and limitations.

## Features

- View the section headers of the executable file.
- Disassemble the text section of the executable file.
- View Dynamic Symbols and PLT table
- Print static ELF metadata and declared dependencies without a terminal.
- Diagnose recursive library resolution, symbol/version requirements and explicit
  CPU ISA/kernel metadata against a target rootfs, with text or JSON output.
- Compare environment reports and import/capture glibc loader evidence.
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
**not assessed**. Use the `diagnose` subcommand for recursive resolution and symbol-version diagnostics.
Missing section metadata is reported as unknown rather than no dependencies.

Reports go to stdout and errors to stderr. Successful inspection exits with 0;
input or mode errors return nonzero. Text output is intended for people. Diagnostic subcommands support versioned JSON:

```bash
ExeViewer diagnose ./app --interactive
ExeViewer diagnose ./app --format json > local.json
ExeViewer diagnose ./app --rootfs ./target-root --target-path /opt/app --format json > target.json
ExeViewer compare --env-a local.json --env-b target.json --format json
ExeViewer trace --log loader.log --format json
ExeViewer trace --execute ./app --output-dir ./capture-001 --timeout 15 --format json
```

Diagnosis exits 0 for no blockers in the checked scope, 1 for blockers, and 2 for
incomplete/unsupported analysis or input errors. Unknown target CPU/kernel or
loader behavior is never presented as proven compatibility. Only explicit
`trace --execute` runs the program. See [CLI details](docs/cli.md) for trace and
comparison exit codes, environment inputs, and supported rules.

The Cargo binary name is `ExeViewer`.


## License

MIT License (c) 2024 sunxfancy
