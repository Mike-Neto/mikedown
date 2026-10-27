#![allow(
    clippy::tests_outside_test_module,
    clippy::unwrap_used,
    clippy::panic,
    reason = "Integration assertions report invalid output and failed filesystem operations"
)]

use mikedown::format;
use unicode_width::UnicodeWidthStr as _;

fn stable(source: &str) -> String {
    let output = format(source).unwrap_or_else(|errors| panic!("{source:?}: {errors:?}"));
    assert_eq!(
        format(&output).unwrap(),
        output,
        "second formatting changed output"
    );
    output
}

#[test]
fn core_blocks_and_nested_lists() {
    let output = stable(
        "#    Heading\n\n@ol start=3\n- first\n    - child\n    - another\n- second\n\n> quoted   words\n> on two lines\n\n---\n",
    );
    assert_eq!(
        output,
        "# Heading\n\n@ol start=3\n3. first\n  a. child\n  b. another\n4. second\n\n> quoted words on two lines\n\n---\n"
    );
    assert_eq!(stable("@ul\n1. a\n2. b\n"), "@ul\n\n1. a\n2. b\n");
}

#[test]
fn lists_renumber_across_digit_boundaries_and_wrap_prefixes() {
    let output = stable(
        "@document mikedown=0.1.0 width=16\n\n@ol start=9\n- one two three four five six\n- seven eight nine ten eleven\n",
    );
    assert!(output.contains("9. one two three\n   four five six"));
    assert!(output.contains("10. seven eight\n    nine ten\n    eleven"));
    assert!(
        output
            .lines()
            .filter(|line| !line.starts_with('@'))
            .all(|line| line.width() <= 16)
    );
}

#[test]
fn wrapping_uses_display_width_and_preserves_indivisible_tokens() {
    let output = stable(
        "@document mikedown=0.1.0 width=10\n\n界界 界界 e\u{301} 👩‍💻 abc\n\nabcdefghijklmnop\n",
    );
    assert!(output.contains("界界 界界\ne\u{301} 👩‍💻 abc"));
    assert!(output.contains("abcdefghijklmnop"));
}

#[test]
fn inline_code_and_code_fences_are_preserved() {
    let source = "@document mikedown=0.1.0 width=12\n\nSome `code  with spaces` and ``a ` b`` end.\n\n````rust\n@unknown\n  untouched   spaces\n```\n````\n";
    let output = stable(source);
    assert!(output.contains("`code  with spaces`"));
    assert!(output.contains("``a ` b``"));
    assert!(output.ends_with("````rust\n@unknown\n  untouched   spaces\n```\n````\n"));
}

#[test]
fn width_off_and_local_overrides() {
    let output = stable(
        "@document mikedown=0.1.0 width=12\n\n@paragraph width=off\nthese lines are much longer than twelve\nkeep this line break\n\n@paragraph width=6\none two three four\n\n@quote width=off\n> keep these physical lines\n> too\n",
    );
    assert!(output.contains("these lines are much longer than twelve\nkeep this line break"));
    assert!(output.contains("one\ntwo\nthree\nfour"));
    assert!(output.contains("> keep these physical lines\n> too"));
    stable(
        "@document mikedown=0.1.0 width=off\n\nfirst line\nsecond line\n\n| header |\n| a long cell value |\n",
    );
}

#[test]
fn quote_paragraphs_and_empty_documents() {
    assert_eq!(stable("\n\n"), "");
    assert_eq!(stable("> first\n>\n> second\n"), "> first\n>\n> second\n");
    assert_eq!(stable(""), "");
}

#[test]
fn all_table_inputs_normalize_to_left_aligned_pipes() {
    let expected =
        "@table\n| Name   | Price |\n| ------ | ----- |\n| Coffee | 3     |\n| Tea    | 2     |\n";
    for source in [
        "| Name | Price |\n| Coffee | 3 |\n| Tea | 2 |\n",
        "┌────────┬───────┐\n│ Name   │ Price │\n├────────┼───────┤\n│ Coffee │ 3     │\n│ Tea    │ 2     │\n└────────┴───────┘\n",
        "+--------+-------+\n| Name   | Price |\n+--------+-------+\n| Coffee | 3     |\n| Tea    | 2     |\n+--------+-------+\n",
        "| Name | Price |\n| :---: | ---: |\n| Coffee | 3 |\n| Tea | 2 |\n",
    ] {
        assert_eq!(stable(&format!("@table\n{source}")), expected);
    }
}

#[test]
fn removed_table_options_have_source_located_errors() {
    for (directive, option, column) in [
        ("@table", "style=unicode", 8),
        ("@table", "style=ascii", 8),
        ("@table", "style=pipes", 8),
        ("@table", "align=left,right", 8),
        ("@document", "table=pipes", 11),
    ] {
        let source = format!("{directive} {option}\n\n| a | b |\n");
        let diagnostic = format(&source).unwrap_err().remove(0);
        assert_eq!((diagnostic.line, diagnostic.column), (1, column));
        assert!(diagnostic.message.contains("unsupported option"));
    }
}

#[test]
fn table_unicode_width_and_escapes_round_trip() {
    let output =
        stable("@table\n| 名 | Cost | Icon |\n| 界界 | 3 | 👩‍💻 |\n| e\u{301} | 20 | 😀 |\n");
    let lines: Vec<_> = output
        .lines()
        .filter(|line| !line.starts_with('@'))
        .collect();
    assert!(
        lines
            .iter()
            .all(|line| line.width() == lines.first().unwrap().width())
    );
    let escaped_output = stable("| h | v |\n| a\\|b | c│d |\n| `x|y` | ``a│`b`` |\n");
    assert!(escaped_output.contains("a\\|b"));
    assert!(escaped_output.contains("c\\│d"));
    assert!(escaped_output.contains("`x\\|y`"));
    assert!(escaped_output.contains("``a\\│`b``"));
    assert_eq!(
        stable("| a | b | c |\n| :--- | ---: | :---: |\n| x | 12 | y |\n"),
        "| a   | b   | c   |\n| --- | --- | --- |\n| x   | 12  | y   |\n"
    );
}

#[test]
fn existing_formatted_tables_can_be_edited_and_reformatted() {
    let output = stable("@table\n| Name |\n| Coffee |\n");
    let edited = output.replace("Coffee", "Espresso coffee");
    let reformatted = stable(&edited);
    assert!(reformatted.contains("Espresso coffee"));
    assert!(!reformatted.contains("Coffee"));
}

#[test]
fn tables_drive_document_width_and_local_settings_win() {
    let input = "@document mikedown=0.1.0 width=12\n\none two three four five six seven eight nine ten\n\n| heading |\n| 12345678901234567890 |\n\n@paragraph width=8\none two three four\n";
    let output = stable(input);
    assert!(output.starts_with("@document mikedown=0.1.0 width=12 wrap=table:table-1"));
    assert!(output.contains("one two three four five\nsix seven eight nine ten"));
    assert!(output.contains("@table id=table-1"));
    assert!(output.contains("@paragraph width=8\none two\nthree\nfour"));
}

#[test]
fn widest_table_selection_is_stable_and_references_refresh() {
    let input = "@document mikedown=0.1.0 width=8\n\n@table id=reserved\n| first |\n| 123456789 |\n\n@table id=chosen\n| second |\n| 123456789012345 |\n";
    let output = stable(input);
    assert!(output.contains("wrap=table:chosen"));
    let shrunk = output.replace("123456789012345", "x");
    let refreshed = stable(&shrunk);
    assert!(refreshed.contains("wrap=table:reserved"));
    let first_wider = output.replace("| 123456789 |", "| 123456789012345678901234567890 |");
    assert!(stable(&first_wider).contains("wrap=table:reserved"));
    let tie = stable(
        "@document mikedown=0.1.0 width=8\n\n| a |\n| 123456789 |\n\n| b |\n| 123456789 |\n",
    );
    assert_eq!(tie.matches("id=table-1").count(), 1);
    assert!(tie.find("id=table-1").unwrap() < tie.find("| a").unwrap());
}

#[test]
fn deleted_target_and_shrinking_tables_restore_minimum() {
    let output = stable(
        "@document mikedown=0.1.0 width=12 wrap=table:deleted\n\nplain words remain here\n\n| h |\n| x |\n",
    );
    assert!(!output.contains("wrap="));
    assert!(output.contains("plain words\nremain here"));
    let generated = stable(
        "@document\n\n| h |\n| 123456789012345678901234567890123456789012345678901234567890123456789012345678901234 |\n",
    );
    assert!(generated.contains("wrap=table:table-1"));
    let mut small = String::new();
    for line in generated.lines() {
        if line.starts_with("| 123") {
            small.push_str("| x |\n");
        } else {
            small.push_str(line);
            small.push('\n');
        }
    }
    assert!(!stable(&small).contains("wrap="));
    let deleted = stable("@document mikedown=0.1.0 wrap=table:table-1\n\nno tables remain\n");
    assert!(!deleted.contains("wrap="));
}

#[test]
fn automatic_ids_skip_existing_ids() {
    let output = stable(
        "@document mikedown=0.1.0 width=8\n\n@table id=table-1\n| x |\n\n| wide |\n| 123456789012345 |\n",
    );
    assert!(output.contains("wrap=table:table-2"));
    assert_eq!(output.matches("id=table-1").count(), 1);
    assert_eq!(output.matches("id=table-2").count(), 1);
}

#[test]
fn malformed_documents_have_source_locations() {
    let cases = [
        ("@ol start\n- item", "expected key=value"),
        ("@ol ---\n- item", "expected key=value"),
        ("@paragraph width=0\ntext", "invalid value"),
        ("@document mikedown=0.1.0 width=abc\ntext", "invalid value"),
        ("@table style=html\n| a |", "unsupported option"),
        ("@table id=1bad\n| a |", "invalid value"),
        ("@ol color=red\n- a", "unsupported option"),
        ("@paragraph width=2 width=3\ntext", "duplicate option"),
        ("@ol", "no target"),
        ("@ol\n@quote\n- a", "no target"),
        ("@ol\nparagraph", "incompatible target"),
        ("text\n\n@document", "before all blocks"),
        ("@document\n@document", "before all blocks"),
        ("@table\n| a | b |\n| x |", "column count"),
        ("@table\n| a", "closing delimiters"),
        ("@table align=right\n| a | b |", "unsupported option"),
        (
            "@table id=x\n| a |\n\n@table id=x\n| b |",
            "duplicate table ID",
        ),
        ("```\nno close", "unclosed code fence"),
        ("@table", "no target"),
    ];
    for (source, expected) in cases {
        let errors = format(source).unwrap_err();
        assert!(errors.first().unwrap().line > 0);
        assert!(errors.first().unwrap().column > 0);
        assert!(
            errors.first().unwrap().message.contains(expected),
            "{source:?}: {errors:?}"
        );
    }
    let diagnostic = format("@paragraph width=no\ntext").unwrap_err().remove(0);
    assert_eq!(diagnostic.column, 12);
    assert_eq!(diagnostic.to_string(), "1:12: invalid value 'no' for width");
}

#[test]
fn wrapping_never_introduces_new_blocks() {
    for token in [
        "#", "###", "-", "+", "*", ">", "---", "123.", "|cell|", "```x```", "~~~x",
    ] {
        stable(&format!(
            "@document mikedown=0.1.0 width=8\n\nhello world {token} word end more text\n"
        ));
    }
    stable(
        "@document mikedown=0.1.0 width=9\n\n- hello world - x more words # heading and |cell| end\n",
    );
    assert!(stable("\\@ol\ntext").ends_with("\\@ol text\n"));
}

#[test]
fn line_endings_are_canonical_and_annotations_sort_options() {
    assert_eq!(
        stable("@document width=80 mikedown=0.1.0\r\n\r\n#  Hi\r\n"),
        "@document mikedown=0.1.0 width=80\n\n# Hi\n"
    );
}

#[test]
fn varied_documents_remain_valid_and_idempotent_after_formatting() {
    // Deterministic corpus of awkward wrap boundaries and mixed block markers.
    // This checks a language invariant rather than any particular layout algorithm.
    let words = [
        "a",
        "longword",
        "界",
        "e\u{301}",
        "😀",
        "#",
        "-",
        ">",
        "---",
        "|x|",
        "+---+",
        "1.",
        "`a b`",
        "\\---",
        "~~~",
        "##",
        "@document",
        "@paragraph",
        "@quote",
        "@ol",
        "@ul",
        "@table",
        "@miguel",
        "@tabletop",
        "\\@ol",
    ];
    let mut random: u64 = 123_456;
    let iterations: usize = 1500;
    let words_per_document: usize = 30;
    let random_shift: u32 = 32;
    for _ in 0..iterations {
        let mut source = String::from("@document mikedown=0.1.0 width=12\n\n");
        for _ in 0..words_per_document {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            #[expect(
                clippy::integer_division_remainder_used,
                reason = "Generated test data uses modulo to select a word and needs no constant-time arithmetic"
            )]
            let index = usize::try_from(random >> random_shift).unwrap() % words.len();
            source.push_str(words.get(index).unwrap());
            source.push(' ');
        }
        if let Ok(first) = format(&source) {
            let second = format(&first).unwrap_or_else(|error| {
                panic!("generated invalid output: {source:?}\n{first}\n{error:?}")
            });
            assert_eq!(second, first, "source: {source}");
        }
    }
}

#[test]
fn complete_document_fixture() {
    let input = include_str!("fixtures/document.input.mkd");
    let expected = include_str!("fixtures/document.expected.mkd");
    assert_eq!(stable(input), expected);
}

#[test]
fn empty_list_items_round_trip() {
    for source in ["@ol\n-\n- second\n", "1.\n2. second\n", "-\n  -\n- end\n"] {
        stable(source);
    }
}

#[test]
fn directives_need_no_closing_delimiter_and_keep_their_scope() {
    let source = "  @document mikedown=0.1.0 width=24\n\n  @paragraph width=off\nthis physical line remains longer than twenty-four\nnext line\n\n@quote width=off\n> quoted first line\n> quoted second line\n\n@ol\n\n- first\n- second\n\n@ul\n1. first\n2. second\n\n@table\n| a |\n| b |\n";
    let output = stable(source);
    assert!(output.starts_with("@document mikedown=0.1.0 width=24\n\n@paragraph width=off\n"));
    assert!(output.contains("twenty-four\nnext line"));
    assert!(output.contains("@quote width=off\n> quoted first line\n> quoted second line"));
    assert!(output.contains("@ol\n1. first\n2. second"));
    assert!(output.contains("@ul\n\n1. first\n2. second"));
    assert!(output.contains("@table\n| a   |\n| --- |\n| b   |"));
}

#[test]
fn unknown_names_mentions_and_name_prefixes_are_prose() {
    for text in [
        "@miguel please review this",
        "@tabl style=unicode",
        "@tabletop",
        "@olivia start=3",
        "@documentary width=80",
        "@paragraphs",
        "@quotes",
        "@ultra",
        "@OL",
        "@ol,",
        "@ol=3",
        "@ol: start=3",
        "@ol@example.com",
        "@",
        "@@ol",
        "@ ol",
        "@{ol}",
    ] {
        assert_eq!(stable(text), format!("{text}\n"));
    }
    assert_eq!(stable("@\tol"), "@ ol\n");
}

#[test]
fn inline_directives_and_escaped_directives_remain_text() {
    for directive in [
        "@document",
        "@paragraph",
        "@quote",
        "@ol",
        "@item",
        "@table",
    ] {
        let inline = format!("Use {directive} to configure the next block.\n");
        assert_eq!(stable(&inline), inline);
        let escaped = format!("\\{directive} width=80\n");
        assert_eq!(stable(&escaped), escaped);
    }
    let code =
        "```text\n@document mikedown=0.1.0 invalid\n@ol\n@table style=invalid\n--- ol ---\n```\n";
    assert_eq!(stable(code), code);
}

#[test]
fn directive_names_cannot_be_activated_by_wrapping() {
    for directive in [
        "@document",
        "@paragraph",
        "@quote",
        "@ol",
        "@item",
        "@table",
    ] {
        for prefix in ["", "- ", "> "] {
            let source =
                format!("@document mikedown=0.1.0 width=8\n\n{prefix}hello {directive} word\n");
            let output = stable(&source);
            assert!(output.contains(&format!("hello {directive}")));
        }
    }
    let output = stable("@document mikedown=0.1.0 width=8\n\nhello @miguel word\n");
    assert!(output.contains("hello\n@miguel\nword"));
}

#[test]
fn old_annotation_notation_is_prose_and_rules_remain_rules() {
    let source = "--- ol ---\n\n- first\n- second\n\n---\n\n--- table style=ascii ---\n";
    assert_eq!(stable(source), source);
    let table = "@table\n| a |\n| --- |\n| b |\n";
    let output = stable(table);
    assert!(output.contains("| --- |"));
    assert_eq!(
        stable("@document mikedown=0.1.0 width=8\n\n--- ol ---\n"),
        "@document mikedown=0.1.0 width=8\n\n--- ol ---\n"
    );
}

#[test]
fn directive_options_have_accurate_source_coordinates() {
    let width_errors = format("\n  @paragraph width=invalid\ntext\n").unwrap_err();
    assert_eq!(width_errors.first().unwrap().line, 2);
    assert_eq!(width_errors.first().unwrap().column, 14);
    let unknown_option_errors = format("@table mystery=value\n| a |\n").unwrap_err();
    assert_eq!(unknown_option_errors.first().unwrap().line, 1);
    assert_eq!(unknown_option_errors.first().unwrap().column, 8);
    assert_eq!(
        unknown_option_errors.first().unwrap().message,
        "unsupported option 'mystery' for table"
    );
    let missing_value_errors = format("@table table\n| a |\n").unwrap_err();
    assert_eq!(missing_value_errors.first().unwrap().column, 8);
    let duplicate_option_errors = format("@paragraph width=2 width=2\ntext\n").unwrap_err();
    assert_eq!(duplicate_option_errors.first().unwrap().column, 20);
    assert_eq!(
        duplicate_option_errors.first().unwrap().message,
        "duplicate option 'width'"
    );
    assert_eq!(
        stable("@paragraph\twidth=off\nfirst\nsecond\n"),
        "@paragraph width=off\nfirst\nsecond\n"
    );
}

#[test]
fn parser_boundaries_preserve_prose_and_block_precedence() {
    for text in [
        "####### text",
        "#text",
        "-\titem",
        "1.\titem",
        ">text",
        ">\u{2003}text",
    ] {
        // These resemble structural markers but do not satisfy their boundaries.
        let expected = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(stable(text), format!("{expected}\n"));
    }
    assert_eq!(stable("#\u{2003}Heading"), "# Heading\n");
    assert_eq!(stable(">\twords"), "> words\n");
    assert_eq!(stable("*\n+\n12.\n"), "-\n-\n12.\n");
    assert_eq!(
        stable("before\n# Heading\nafter"),
        "before\n\n# Heading\n\nafter\n"
    );
    assert_eq!(stable("\t- item\n"), "\t- item\n");
}

#[test]
fn unicode_annotation_spacing_keeps_scalar_columns() {
    assert_eq!(
        stable("\u{2003}@paragraph\u{a0}width=off\u{2003}\nfirst\nsecond\n"),
        "@paragraph width=off\nfirst\nsecond\n"
    );
    for (source, column, message) in [
        (
            "\u{2003}@paragraph\u{a0}width=invalid\ntext",
            13,
            "invalid value 'invalid' for width",
        ),
        ("@table\u{2003}id=é\n| h |", 8, "invalid value 'é' for id"),
        (
            "@table\u{2003}id=ok\u{a0}id=other\n| h |",
            14,
            "duplicate option 'id'",
        ),
        ("@paragraph width=\ntext", 12, "invalid value '' for width"),
        (
            "@paragraph width=2=3\ntext",
            12,
            "invalid value '2=3' for width",
        ),
    ] {
        let diagnostic = format(source).unwrap_err().remove(0);
        assert_eq!(diagnostic.line, 1, "{source:?}");
        assert_eq!(diagnostic.column, column, "{source:?}");
        assert_eq!(diagnostic.message, message, "{source:?}");
    }
}

#[test]
fn fences_require_matching_characters_lengths_and_clean_closers() {
    let source = "````text\n```\n~~~~\n```` trailing\n@ol\n`````\n";
    assert_eq!(stable(source), source);
    for malformed_fence in ["```", "~~~~\n~~~", "````\n```", "```\n~~~", "```\n```text"] {
        let diagnostic = format(malformed_fence).unwrap_err().remove(0);
        assert_eq!(diagnostic.to_string(), "1:1: unclosed code fence");
    }
}

#[test]
fn inline_runs_preserve_unmatched_spans_and_prose_backslashes() {
    assert_eq!(
        stable("@document mikedown=0.1.0 width=8\n\na `b  c\nd  e\n"),
        "@document mikedown=0.1.0 width=8\n\na `b c d\ne\n"
    );
    assert_eq!(
        stable("@document mikedown=0.1.0 width=8\n\na \\`b  c` d\n"),
        "@document mikedown=0.1.0 width=8\n\na\n\\`b  c`\nd\n"
    );
    assert_eq!(
        stable("@document mikedown=0.1.0 width=8\n\na ``b ` c`` d\n"),
        "@document mikedown=0.1.0 width=8\n\na\n``b ` c``\nd\n"
    );
}

#[test]
fn table_parsing_preserves_empty_cells_escapes_and_backtick_runs() {
    for row in [
        "| | |",
        "| \\| | \\│ |",
        "| \\界 | trailing\\ |",
        "| ``a ` | b`` | c |",
        "| `a\\`|b` | c |",
        "| a | `unclosed | still one cell |",
        "│ a|b │ `x│y` │",
    ] {
        let source = format!("@table\n{row}\n");
        let output = stable(&source);
        assert!(output.contains("| ---"), "{row:?}: {output}");
    }
    for (source, message) in [
        ("|", "table rows must have opening and closing delimiters"),
        ("│", "table rows must have opening and closing delimiters"),
        (
            "│ a |",
            "table rows must have opening and closing delimiters",
        ),
        ("┌──┤\n│ a │", "malformed table border"),
        ("+---\n| a |", "malformed table border"),
        ("┌─┐", "table has no header row"),
        ("| --- |", "table separator must follow the header"),
        (
            "| a |\n| --- |\n| --- |",
            "table separator must follow the header",
        ),
    ] {
        let diagnostic = format(&format!("@table\n{source}")).unwrap_err().remove(0);
        assert_eq!(diagnostic.message, message, "{source:?}");
        assert_eq!(diagnostic.column, 1);
    }
}

#[test]
fn document_and_version_declarations_are_optional() {
    assert_eq!(stable("#    Notes\n"), "# Notes\n");
    assert_eq!(stable("@document\n\n# Notes\n"), "@document\n\n# Notes\n");
    assert_eq!(
        stable("@document mikedown=0.1.0\n\n# Notes\n"),
        "@document mikedown=0.1.0\n\n# Notes\n"
    );
    assert_eq!(
        stable("@document width=12\n\none two three four\n"),
        "@document width=12\n\none two\nthree four\n"
    );
    assert_eq!(stable("@document"), "@document\n");
    for source in ["", "\n\n", " \t\r\n"] {
        assert_eq!(stable(source), "");
    }
}

#[test]
fn wide_tables_do_not_require_a_document_declaration() {
    let source = "| header |\n| 123456789012345678901234567890123456789012345678901234567890123456789012345678901234 |\n";
    let output = stable(source);
    assert!(!output.contains("@document"));
    assert!(!output.contains("wrap="));
    assert!(!output.contains("id="));
    assert!(output.starts_with('|'));
}

#[test]
fn document_versions_have_source_located_validation() {
    for version in ["", "0.2.0", "1.0.0", "0.1", "v0.1.0", "0.1.0-extra"] {
        let diagnostic = format(&format!("\n  @document mikedown={version}\n\n# Notes\n"))
            .unwrap_err()
            .remove(0);
        assert_eq!(diagnostic.line, 2);
        assert_eq!(diagnostic.column, 13);
        assert_eq!(
            diagnostic.message,
            format!("unsupported Mikedown version '{version}'; supported version is 0.1.0")
        );
    }
    let diagnostic = format("@document mikedown=0.1.0 mikedown=0.1.0\ntext")
        .unwrap_err()
        .remove(0);
    assert_eq!(diagnostic.column, 26);
    assert_eq!(diagnostic.message, "duplicate option 'mikedown'");
    let paragraph_diagnostic = format("@paragraph mikedown=0.1.0\ntext")
        .unwrap_err()
        .remove(0);
    assert_eq!(
        paragraph_diagnostic.message,
        "unsupported option 'mikedown' for paragraph"
    );
}

#[test]
fn version_declarations_inside_code_remain_literal() {
    let code = "```text\n@document mikedown=999.0.0\n```\n";
    assert_eq!(stable(code), code);
}

#[test]
fn markdown_preservation_golden_fixtures() {
    assert_eq!(
        stable(include_str!("fixtures/front-matter.input.md")),
        include_str!("fixtures/front-matter.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/html.input.md")),
        include_str!("fixtures/html.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/references.input.md")),
        include_str!("fixtures/references.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/indented.input.md")),
        include_str!("fixtures/indented.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/links.input.md")),
        include_str!("fixtures/links.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/hard-breaks.input.md")),
        include_str!("fixtures/hard-breaks.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/setext.input.md")),
        include_str!("fixtures/setext.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/rules.input.md")),
        include_str!("fixtures/rules.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/list-starts.input.md")),
        include_str!("fixtures/list-starts.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/pipe-tables.input.md")),
        include_str!("fixtures/pipe-tables.expected.md")
    );
    assert_eq!(
        stable(include_str!("fixtures/irregular-table.input.md")),
        include_str!("fixtures/irregular-table.expected.md")
    );
}

#[test]
fn markdown_precedence_and_overrides() {
    assert_eq!(
        stable("---\nunclosed:  yaml\n@document width=bad\n"),
        "---\nunclosed:  yaml\n@document width=bad\n"
    );
    assert_eq!(stable("\n---\n"), "---\n");
    assert_eq!(stable("text\n    ```\n"), "text ```\n");
    assert_eq!(
        stable("@ol start=2\n7. first\n9. second\n"),
        "@ol start=2\n2. first\n3. second\n"
    );
    let output = stable("@table\n| h | v |\n| --- | ---: |\n| a | b |\n");
    assert_eq!(
        output,
        "@table\n| h   | v   |\n| --- | --- |\n| a   | b   |\n"
    );
}

#[test]
fn hard_break_segments_wrap_independently_and_width_off_keeps_markers() {
    assert_eq!(
        stable("@document width=8\n\none two three  \nfour five six\\\nseven eight\n"),
        "@document width=8\n\none two\nthree  \nfour\nfive six\\\nseven\neight\n"
    );
    let source = "@paragraph width=off\none   two  \nthree\\\nfour\n";
    assert_eq!(stable(source), source);
}

#[test]
fn markdown_table_lookahead_preserves_content_and_strict_annotations() {
    assert_eq!(stable("a | b\nprose follows\n"), "a | b prose follows\n");
    for source in [
        "| a | b\n| --- | ---\n| x | y\n",
        "a | b |\n--- | --- |\nx | y |\n",
        "| | b |\n| --- | --- |\n| | y |\n",
    ] {
        assert!(stable(source).contains("| --- | --- |"));
    }
    let source = include_str!("fixtures/irregular-table.input.md");
    let diagnostic = format(&format!("@table\n{source}")).unwrap_err().remove(0);
    assert_eq!(diagnostic.line, 4);
    assert!(diagnostic.message.contains("column count"));
    assert_eq!(
        stable("text\n<span>\nmore   words\n"),
        "text <span> more words\n"
    );
}
