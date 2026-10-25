//! Ordered-list labels are assigned before references and layout.

use alloc::collections::BTreeMap;

use crate::Annotation;
use crate::parser::ListItem;

pub(crate) const GREEK: &str = "αβγδεζηθικλμνξοπρστυφχψω";

pub(crate) fn assign(items: &mut [ListItem], annotation: Option<&Annotation>) {
    let cycle = annotation.is_some_and(|value| value.name == "ol");
    let root = items.first().map_or(0, |item| item.indent);
    let mut numbers = BTreeMap::new();
    let mut ancestors: Vec<String> = Vec::new();
    for item in items {
        numbers.retain(|indent, _| *indent <= item.indent);
        let at_root = item.indent == root;
        let ordered = cycle || item.ordered;
        if ordered {
            let start = if at_root {
                annotation
                    .and_then(|value| value.options.get("start"))
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(item.start)
            } else if cycle {
                1
            } else {
                item.start
            };
            let number = numbers.entry(item.indent).or_insert(start);
            let depth = item.depth;
            ancestors.truncate(depth);
            let label = if cycle {
                marker(depth, *number)
            } else {
                number.to_string()
            };
            item.marker = format!("{label}.");
            ancestors.push(label);
            item.path = ancestors.join(".");
            *number = number.saturating_add(1);
        } else {
            numbers.remove(&item.indent);
            item.marker = "-".into();
        }
    }
}

fn marker(depth: usize, number: usize) -> String {
    match depth & 3 {
        0 => number.to_string(),
        1 => alphabetic(number, "abcdefghijklmnopqrstuvwxyz"),
        2 => roman(number),
        _ => alphabetic(number, GREEK),
    }
}

#[expect(
    clippy::integer_division_remainder_used,
    reason = "Bijective alphabetic numbering uses public list ordinals, not secret data"
)]
fn alphabetic(mut number: usize, alphabet: &str) -> String {
    let letters: Vec<char> = alphabet.chars().collect();
    let mut output = Vec::new();
    while number > 0 {
        number -= 1;
        output.push(letters[number % letters.len()]);
        number /= letters.len();
    }
    output.into_iter().rev().collect()
}

fn roman(mut number: usize) -> String {
    let mut output = String::new();
    for (value, digits) in [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while number >= value {
            output.push_str(digits);
            number -= value;
        }
    }
    output
}
