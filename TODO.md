@document width=120

# Mikedown roadmap

The spec, formatter CLI, and Rust core are v0.1. These are implementation notes for later milestones, not features
included in the binary today.

## Shared editor behavior

@ol
- [ ] 1. Consume the Rust formatter as the single formatting authority. Keep the existing whole-document string API; add
         structured text edits and UTF-16 range conversion when editor protocols need them.
- [ ] 2. Reuse the formatter's conformance fixtures in both plugin test suites.
- [ ] 3. Surface source diagnostics without changing invalid buffers.
- [ ] 4. Apply a format in one undo operation and map selections/cursors through edits.
- [ ] 5. Provide an explicit format command first; make format-on-save optional.
- [ ] 6. Keep annotations and real text visible. Syntax highlighting and navigation can help, but reading a document
         must not require a rendered preview.
- [ ] 7. Publish versioned binaries and checksums for supported desktop platforms.

## Zed extension

@ol
- [ ] 1. Build a Tree-sitter grammar for blocks and annotations, including all three table styles and literal code. Add
         highlighting, outlines, folding, and code fence injections. Associate `.mkd` with Mikedown.
- [ ] 2. Match only defined `@name` directives with whitespace/end-of-line name boundaries; leave mentions and unknown
         names as prose in both editor grammars.
- [ ] 3. Register a development extension and document using `mikedown fmt -` as Zed's external formatter. Use the
         existing stdout contract.
- [ ] 4. Add a small LSP wrapper around the Rust core for whole-document formatting and diagnostics, then support
         minimal edit ranges and annotation completions.
- [ ] 5. Implement extension binary discovery and versioned downloads; do not embed a second formatter implementation in
         extension code.
- [ ] 6. Test formatting, invalid buffers, table references, save/undo, and Unicode selection mapping inside Zed.
         Publish the grammar revision and extension only after the CLI's syntax and output are stable.

Reference: https://zed.dev/docs/extensions/languages

## Obsidian plugin

@ol
- [ ] 1. Prototype a dedicated `.mkd` text view and file registration. Check links, file renames, search, and vault
         behavior rather than assuming Markdown views will accept an independent language. Also recognize `.md` files
         with a supported `@document mikedown=VERSION` declaration.

Reference: https://github.com/obsidianmd/obsidian-api

## Later language and tooling work

@ol
- [ ] 1. Audit `arithmetic_side_effects`, `indexing_slicing`, and `string_slice` allowances. Verify arithmetic bounds
         and UTF-8 boundaries before narrowing the exceptions.
