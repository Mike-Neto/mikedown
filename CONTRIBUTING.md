# Development

Run `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets` before
submitting changes. With cached dependencies, Cargo commands support `--offline`.

## Architecture

- `src/parser.rs`: nom recognizers for source-located blocks and annotations,
  with explicit state for annotation scope and list nesting. Complete-input
  parsers preserve language diagnostics; parsing never mutates files.
- `src/inline.rs`: shared nom recognition of backtick runs and table escapes,
  used by prose tokenization, table cells, and delimiter escaping.
- `src/table.rs`: nom table input and border recognition, alignment validation,
  and all three text layouts.
- `src/lib.rs`: document defaults, table measurement/reference maintenance, prose
  wrapping, and canonical emission. This is the shared core for future plugins.
- `src/main.rs`: clap derive argument handling, UTF-8 I/O, batch validation,
  staged atomic replacement, and exit codes. No language rules belong here.
- `src/cli_error.rs`: typed runtime failures derived with thiserror. Preserve I/O
  sources, file paths, diagnostics, and individual batch errors until rendering.
  Library `Diagnostic` also derives thiserror while retaining its public fields.

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
