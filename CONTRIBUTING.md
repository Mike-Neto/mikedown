@document width=120

# Development

Run `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets` before submitting changes. With cached
dependencies, Cargo commands support `--offline`.

## Architecture

@ol
1. `src/parser.rs`: nom recognizers for source-located blocks and annotations, with explicit state for annotation scope
   and list nesting. Complete-input parsers preserve language diagnostics; parsing never mutates files.
2. `src/inline.rs`: shared nom recognition of backtick runs and table escapes, used by prose tokenization, table cells,
   and delimiter escaping.
3. `src/table.rs`: nom table input and border recognition, alignment validation, and all three text layouts.
4. `src/lib.rs`: document defaults, table measurement/reference maintenance, prose wrapping, and canonical emission.
   This is the shared core for future plugins.
5. `src/main.rs`: clap derive argument handling, UTF-8 I/O, batch validation, staged atomic replacement, and exit codes.
   No language rules belong here.
6. `src/cli_error.rs`: typed runtime failures derived with thiserror. Preserve I/O sources, file paths, diagnostics, and
   individual batch errors until rendering. Library `Diagnostic` also derives thiserror while retaining its public fields.
7. `src/list.rs`: assign list markers and full paths before inline references and layout. Shared labels keep emitted
   lists and generated references in sync.
8. `src/text.rs`: retain original inline source coordinates, validate item reference targets, and update generated
   labels before table measurement.

Spec changes need an example and a regression test. For every new construct, verify that formatted text can be edited
and parsed again. In particular, `format(format(source)) == format(source)` must hold, generated metadata must
stabilize, and inline text must not become a new block after wrapping.

Golden fixtures in `tests/fixtures` contain independent expected output. Additional integration tests cover Unicode
width, source diagnostics, generated IDs, and filesystem behavior. Tests use temporary directories outside the
repository.

## Lint conventions

The starter enables Clippy's pedantic, nursery, and restriction groups. We retain those groups, with explicit exceptions
for mutually competing style restrictions and ordinary parser operations. Unicode literals are intentional language
data; validated UTF-8 slicing and bounded arithmetic implement syntax; standard Rust returns, `?`, and scoped visibility
improve readability. Integration tests may unwrap/panic to report failed assertions, and only the CLI prints to stdio.

Integer division and remainder checks, discarded-result checks, explicit final `else` branches, and `alloc`/`core`
import preferences remain enabled. Import collections from `alloc` and shared primitives from `core` when available.
Intentional division and remainder operations use narrow `#[expect]` attributes with reasons. Best-effort destructor
cleanup uses explicitly typed results and local expectations for discarding them. Keep exceptions close to the operation
so future code remains checked.
