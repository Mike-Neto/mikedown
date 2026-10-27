//! Shared alphabetic numbering and the Greek marker alphabet.

/// Greek alphabet used by the fourth ordered-list marker style.
pub const GREEK: &str = "αβγδεζηθικλμνξοπρστυφχψω";

/// Render a positive ordinal using bijective numbering over a nonempty alphabet.
pub fn alphabetic(mut number: usize, alphabet: &str) -> String {
    let letters: Vec<char> = alphabet.chars().collect();
    let mut output: Vec<char> = Vec::new();
    while number > 0 {
        number = number.strict_sub(1);
        // Callers supply fixed, nonempty alphabets; the remainder is a valid letter index.
        let index = number.strict_rem(letters.len());
        output.extend(letters.get(index));
        number = number.strict_div(letters.len());
    }
    output.into_iter().rev().collect()
}
