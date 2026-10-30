# Testing ExeViewer

The supported diagnostic test platform is Linux x86-64 with glibc. Install a Rust
stable toolchain, GCC and Python 3. Fixtures need `/lib64/ld-linux-x86-64.so.2`.
They compile their own small libraries and use temporary directories; target code
is only executed in explicit trace/loader-oracle tests using trusted fixtures.

## Run locally

```sh
cargo test --locked
cargo test --locked --release
cargo clippy --locked --all-targets -- -D clippy::correctness
python3 tests/check_docs.py
```

`cargo test` includes both Python suites via `diagnose_cli.rs`; no separate step is
needed. For a focused run after building the CLI:

```sh
python3 tests/regression_scenarios.py target/debug/ExeViewer -v
python3 tests/regression_scenarios.py target/debug/ExeViewer \
  RegressionTests.test_snapshot_records_missing_interpreter
```

## Coverage

| Suite | Cases | Purpose |
| --- | --- | --- |
| `report_cli.rs` | 6 | Legacy reporting, output streams, redirection, invalid input and terminal requirements |
| `diagnose_scenarios.py` | 43 | Dependency search, RPATH/RUNPATH, versions, symbol providers, rootfs isolation, capture, comparison, repair and TUI; includes real glibc oracles |
| `regression_scenarios.py` | 12 | Snapshot identity/schema/link drift, missing interpreter evidence, repair isolation and new dependencies, trace outcomes/provenance/size limits, corrupt ELF handling |

The corruption tests include nine deterministic malformed-header cases tested through
both `inspect` and `diagnose`, plus 32 reproducible seeded mutations. Each subprocess
has a timeout; malformed inputs are inspected, never executed. Mutations may still
be valid ELF, so their contract is structured output and no crash, not universal rejection.
This is a bounded regression corpus, not continuous fuzzing or a full parser security audit.

Fixture directories are automatically cleaned up. Subtest failures identify the case
or seed/index; re-run the named test to reproduce it. Tests do not depend on ordering.

## CI

Pull requests, pushes to `master`, and manual dispatch run:

- Ubuntu 22.04 and 24.04 × debug and release (four jobs), with fail-fast disabled.
- All 61 test cases and the README end-to-end demo in each matrix job.
- A separate Clippy correctness gate and local documentation/image-link checks.
- Test/lint logs and generated demo reports uploaded for seven days, including on failure.

Rust dependencies are locked and cached. New commits cancel stale runs for the same
PR/ref. Jobs have explicit timeouts and read-only repository permissions.
Existing compiler/style warnings remain visible; the gate denies Clippy correctness
findings rather than suppressing warnings or requiring a broad legacy-formatting change.

The [README demo](../docs/demo/README.md) also asserts its expected exit codes and that
the original B library remains unchanged. CI does not regenerate screenshots or require
Chromium. The documentation check validates local inline links and all eight README
image references; it does not claim to validate external URLs or Markdown anchors.
