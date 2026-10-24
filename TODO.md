# Mikedown roadmap

The spec, formatter CLI, and Rust core are v0.1. These are implementation notes
for later milestones, not features included in the binary today.

## Shared editor behavior

- [ ] Consume the Rust formatter as the single formatting authority. Keep the
  existing whole-document string API; add structured text edits and UTF-16 range
  conversion when editor protocols need them.
- [ ] Reuse the formatter's conformance fixtures in both plugin test suites.
- [ ] Surface source diagnostics without changing invalid buffers.
- [ ] Apply a format in one undo operation and map selections/cursors through edits.
- [ ] Provide an explicit format command first; make format-on-save optional.
- [ ] Keep annotations and real text visible. Syntax highlighting and navigation
  can help, but reading a document must not require a rendered preview.
- [ ] Publish versioned binaries and checksums for supported desktop platforms.

## Zed extension

- [ ] Build a Tree-sitter grammar for blocks and annotations, including all three
  table styles and literal code. Add highlighting, outlines, folding, and code
  fence injections. Associate `.mkd` with Mikedown.
- [ ] Match only defined `@name` directives with whitespace/end-of-line name
  boundaries; leave mentions and unknown names as prose in both editor grammars.
- [ ] Register a development extension and document using
  `mikedown fmt -` as Zed's external formatter. Use the existing stdout contract.
- [ ] Add a small LSP wrapper around the Rust core for whole-document formatting
  and diagnostics, then support minimal edit ranges and annotation completions.
- [ ] Implement extension binary discovery and versioned downloads; do not embed
  a second formatter implementation in extension code.
- [ ] Test formatting, invalid buffers, table references, save/undo, and Unicode
  selection mapping inside Zed. Publish the grammar revision and extension only
  after the CLI's syntax and output are stable.

Reference: https://zed.dev/docs/extensions/languages

## Obsidian plugin

- [ ] Prototype a dedicated `.mkd` text view and file registration. Check links,
  file renames, search, and vault behavior rather than assuming Markdown views
  will accept an independent language. Also recognize `.md` files with a
  supported `@document mikedown=VERSION` declaration.
- [ ] Add CodeMirror 6 highlighting for blocks, annotations, and table IDs.
- [ ] Compile the Rust formatting library to WASM with a small JavaScript binding
  for formatted text and diagnostics. Keep filesystem work outside the core.
- [ ] Bundle the WASM module and test initialization and failures on desktop and
  mobile; avoid depending on a locally installed CLI for mobile use.
- [ ] Add command-palette actions for format document, ordered/unordered lists,
  table style, and document width, using the shared formatter after edits.
- [ ] Apply formatting as one editor transaction, preserving dirty-buffer changes,
  selections, and undo history. Avoid vault-wide automatic rewrites.
- [ ] Test desktop/mobile editing, sync conflicts, IME input, large documents,
  Unicode tables, ID/reference maintenance, and plugin unload/reload.
- [ ] Document v0.1 restrictions and investigate Markdown-note interoperability
  separately from native `.mkd` files.

Reference: https://github.com/obsidianmd/obsidian-api

## Later language and tooling work

- [ ] Editor-assisted editing of wide Unicode tables and navigable width targets.
- [ ] Multi-paragraph and mixed-block list items; independent nested annotations.
- [ ] Optional wrapping within table cells with an unambiguous round-trip grammar.
- [ ] Additional heading styles and generated contents lists.
- [ ] Incremental parsing and formatter benchmarks on large documents.
- [ ] Additional diagnostics per parse and machine-readable CLI diagnostics.
- [ ] Project-level defaults and a versioned language evolution policy.
- [ ] CI matrix and downloadable releases for macOS, Linux, and Windows; v0.1 is
  currently verified locally on macOS.
