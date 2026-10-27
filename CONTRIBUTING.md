@document width=120

# Development

Run `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets` before submitting changes. With cached
dependencies, Cargo commands support `--offline`.

## Architecture

@ol
1. `src/parser.rs`: shared nom recognizers, list parsing, and annotation validation. Complete-input parsers preserve
   language diagnostics; parsing never mutates files.
2. `src/inline.rs`: shared nom recognition of backtick runs and table escapes, used by prose tokenization, table cells,
   and delimiter escaping.
3. `src/table.rs`: shared table recognizers, alignment validation, and all three text layouts.
4. `src/lib.rs`: scoped document and table parsing, list-label assignment, reference maintenance, table measurement,
   prose wrapping, and canonical emission. This is the shared core for future plugins.
5. `src/main.rs`: clap derive argument handling, UTF-8 I/O, batch validation, staged atomic replacement, and exit codes.
   No language rules belong here.
6. `src/cli_error.rs`: typed runtime failures derived with thiserror. Preserve I/O sources, file paths, diagnostics, and
   individual batch errors until rendering. Library `Diagnostic` also derives thiserror while retaining its public fields.
7. `src/list.rs`: shared alphabetic numbering and Greek marker alphabet. The document pipeline assigns list markers and
   full paths before resolving references and measuring layout.
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
for mutually competing style restrictions. Unicode literals are intentional language data; standard Rust returns and `?`
improve readability. Integration tests may unwrap/panic to report failed assertions, and only the CLI prints to stdio.

Document private items, including source-coordinate units and parser invariants. Use iterators, zipped collections, and
checked lookups instead of direct indexing. Split strings only at boundaries derived from recognized spans or complete
Unicode scalars. Use checked arithmetic when recognition can fail and strict arithmetic for operations bounded by
validated input or measured output; avoid silent saturation or wrapping unless the algorithm requires it. Give numeric
values explicit types when inference would fall back, distinguish unrelated shadowed bindings, and import method-only
traits with `as _`. Items in private modules use `pub`; their parent modules still limit external visibility.

Integer division and remainder checks, discarded-result checks, explicit final `else` branches, and `alloc`/`core`
import preferences remain enabled. Import collections from `alloc` and shared primitives from `core` when available.
Intentional division and remainder operations use narrow `#[expect]` attributes with reasons. Best-effort destructor
cleanup uses explicitly typed results and local expectations for discarding them. Keep exceptions close to the operation
so future code remains checked.

Single-use functions stay inline at their call sites. Use comments and scoped blocks to name parsing, validation, and
layout stages. Keep error boundaries local where failures must be collected or syntax must fall back to literal text.
The resulting larger functions use scoped size and complexity expectations while `single_call_fn` remains enabled.
