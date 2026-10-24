# Mikedown

Plain-text documents with formatting instructions. The formatter writes numbers,
spacing, wrapping, and table borders into the file itself, so the result is
readable in a terminal or any text editor. There is no preview or rendering step.

Mikedown borrows familiar Markdown syntax but is an independent language. Its
canonical files use `.mike`; the CLI accepts any filename.

## Try it

Requires Rust 1.96 or later.

```sh
cargo build --release
./target/release/mikedown fmt examples/tour.mike
./target/release/mikedown fmt --write notes.mike
./target/release/mikedown fmt --check notes.mike
cat notes.mike | ./target/release/mikedown fmt
```

Install locally with `cargo install --path .`. For a checkout with dependencies
already cached, add `--offline` to Cargo commands.

## Write ordinary text, add instructions when needed

```text
@document width=80 table=unicode

# Morning routine

@ol
- Buy beans
- Make coffee

@table
| Drink | Price |
| Coffee | 3 |
| Tea | 2 |
```

Formatting saves this:

```text
@document table=unicode width=80

# Morning routine

@ol
1. Buy beans
2. Make coffee

@table
┌────────┬───────┐
│ Drink  │ Price │
├────────┼───────┤
│ Coffee │ 3     │
│ Tea    │ 2     │
└────────┴───────┘
```

Annotations configure the next block. Ordinary headings, lists, quotes, paragraphs,
and tables also format without annotations. Select `style=ascii` or `style=pipes`
on a table, use `align=left,right` for its columns, or set `width=off` on a prose
block to preserve its physical line breaks.

Only defined directive names are recognized at the beginning of a line.
`@miguel`, `@tabletop`, and other unknown names stay ordinary prose. To write a
literal known directive, use `\@ol`; its backslash remains visible. The former
`--- ol ---` notation is now ordinary prose, while `---` remains a horizontal rule.

Prose wraps at 80 displayed columns by default. A wider table becomes the
document's wrapping target: the formatter adds a table ID and a leading
`wrap=table:ID` reference. Other prose then follows that table's measured width.
The widest table wins, and the configured width remains the minimum. See
[the width-target example](examples/width-target.mike).

Formatted output stays editable input. Change a cell directly inside a Unicode
table and format again; borders and column padding are recalculated. A second
format without edits produces identical bytes.

## Library

```rust
let source = "@ol\n- Buy beans\n- Make coffee\n";
let formatted = mikedown::format(source).expect("valid Mikedown");
assert_eq!(formatted, "@ol\n1. Buy beans\n2. Make coffee\n");
```

`format(&str)` returns `Result<String, Vec<Diagnostic>>`. Diagnostics contain
one-based source line and column numbers. The library has no filesystem or editor
dependencies. The current parser returns the first document error.

## Status and development

v0.1 implements the spec, Rust library, formatter binary, and examples. Zed and
Obsidian plugins are planned in [TODO.md](TODO.md); they are not implemented yet.
See [SPEC.md](SPEC.md) for the complete syntax and formatting rules.

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets
```

Tests cover table round trips, Unicode widths, nested lists, wrapping stability,
CLI pipelines, atomic replacements, and batch validation. See
[CONTRIBUTING.md](CONTRIBUTING.md) for architecture and lint conventions.
