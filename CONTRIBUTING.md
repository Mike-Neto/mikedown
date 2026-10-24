# Development

Run `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets` before
submitting changes. With cached dependencies, Cargo commands support `--offline`.

## Architecture

- `src/parser.rs`: source-located block and annotation parsing, including literal
  code protection. Parsing never mutates files.
- `src/table.rs`: table input, alignment, escaping, and all three text layouts.
- `src/lib.rs`: document defaults, table measurement/reference maintenance, prose
  wrapping, and canonical emission. This is the shared core for future plugins.
- `src/main.rs`: argument handling, UTF-8 I/O, batch validation, staged atomic
  replacement, and exit codes. No language rules belong here.

Spec changes need an example and a regression test. For every new construct,
verify that formatted text can be edited and parsed again. In particular,
`format(format(source)) == format(source)` must hold, generated metadata must
stabilize, and inline text must not become a new block after wrapping.

Golden fixtures in `tests/fixtures` contain independent expected output. Additional
integration tests cover Unicode width, source diagnostics, generated IDs, and
filesystem behavior. Tests use temporary directories outside the repository.

## Lint conventions

The starter enables Clippy's pedantic, nursery, and restriction groups. We retain
those groups, with explicit exceptions for mutually competing style restrictions
and ordinary parser operations. Unicode literals are intentional language data;
validated UTF-8 slicing and bounded arithmetic implement syntax; standard Rust
returns, `?`, and scoped visibility improve readability. Integration tests may
unwrap/panic to report failed assertions, and only the CLI prints to stdio.
