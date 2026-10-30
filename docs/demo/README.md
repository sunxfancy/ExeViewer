# Reproduce the README demo

Requirements: Linux x86-64 with glibc, GCC, Python 3, and a built `ExeViewer` containing
`compare --explain` and `diagnose --overrides`.

```sh
cargo build --locked
python3 docs/demo/run.py --binary target/debug/ExeViewer --work-dir /tmp/exeviewer-demo
```

The work directory must not exist. The script builds a trusted executable requiring
`FOO_2.0`, two small rootfs fixtures containing `libfoo.so` at the same path, and a
repair configuration. A provides `FOO_2.0`; B only provides `FOO_1.0`.
It asserts exit codes 0 (A), 1 (B), 1 (comparison), and 0 (static repair verification).
The executable is never run. The original B library is not replaced.

The directory retains full JSON reports, source files, transcript excerpts, and
`verification/` with the before/after reports and replacement copy. In displayed
commands, `$DEMO` is this work directory. To rerun a repair manually, use a new
`--verification-dir`.

The README images are Chromium screenshots of **selected fields from these actual
JSON results**, rendered for readability. They are not screenshots of a new GUI
or a verbatim full terminal session. No diagnostic values are hand-authored.
Captured text alternatives are in [transcripts](transcripts/).

Optional image regeneration requires the Python `playwright` package and Chromium:

```sh
python3 docs/demo/render.py --work-dir /tmp/exeviewer-demo --output-dir docs/images
cp /tmp/exeviewer-demo/0*.txt docs/demo/transcripts/
```

`--chromium /path/to/chromium` selects an installed browser. If omitted and Chromium
is not on PATH, Playwright uses its own installed browser. Images use local fonts
and do not load external resources. Hashes can differ with toolchain versions;
regenerate images and transcripts together.
