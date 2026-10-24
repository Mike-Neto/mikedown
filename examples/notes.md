@document mikedown=0.1.0 table=pipes width=80

# Markdown filename, Mikedown formatting

Save a Mikedown document with the .md extension and format it with the same CLI.
The document declaration records the Mikedown version used by the formatter.

@table style=pipes
| Command                       | Purpose          |
| ----------------------------- | ---------------- |
| mikedown fmt notes.md         | Print formatted  |
| mikedown fmt --check notes.md | Check formatting |
| mikedown fmt --write notes.md | Save formatted   |
