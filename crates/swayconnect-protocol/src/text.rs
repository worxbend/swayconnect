//! Text rules of `docs/PROTOCOL.md` 2.4 and the closed character sets of 5.2.

use crate::limits::MAX_NAME_BYTES;

/// The fixed list of spec 2.4. It does not follow later Unicode versions.
pub fn is_control_or_format(character: char) -> bool {
    matches!(
        character,
        '\u{0000}'..='\u{001F}'
            | '\u{007F}'..='\u{009F}'
            | '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// Replaces each control or format character with U+FFFD (spec 9.3 rule 3).
pub fn replace_unsafe(text: &str) -> String {
    text.chars()
        .map(|character| {
            if is_control_or_format(character) {
                char::REPLACEMENT_CHARACTER
            } else {
                character
            }
        })
        .collect()
}

/// Renders each control or format character as `\u{..}`, for terminals and log lines.
pub fn escape_for_terminal(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if is_control_or_format(character) {
            escaped.push_str(&format!("\\u{{{:x}}}", u32::from(character)));
        } else {
            escaped.push(character);
        }
    }
    escaped
}

/// Rule of `device_name`, `server.name` and the URI `name`: 1 to 64 bytes, no control or
/// format character.
pub fn is_display_name(text: &str) -> bool {
    (1..=MAX_NAME_BYTES).contains(&text.len()) && !text.chars().any(is_control_or_format)
}

/// Rule of `platform` and `client`: 1 to `max_bytes` bytes of `A-Z a-z 0-9 . _ / -` and space.
pub fn is_info_text(text: &str, max_bytes: usize) -> bool {
    (1..=max_bytes).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._/- ".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every range of spec 2.4, first and last scalar value.
    const RANGES: [(u32, u32); 11] = [
        (0x0000, 0x001F),
        (0x007F, 0x009F),
        (0x00AD, 0x00AD),
        (0x061C, 0x061C),
        (0x180E, 0x180E),
        (0x200B, 0x200F),
        (0x2028, 0x202E),
        (0x2060, 0x206F),
        (0xFEFF, 0xFEFF),
        (0xFFF9, 0xFFFB),
        (0xE0000, 0xE007F),
    ];

    fn scalar(code_point: u32) -> char {
        char::from_u32(code_point).expect("test code point is a scalar value")
    }

    #[test]
    fn every_range_is_control_or_format_and_its_neighbours_are_not() {
        for (first, last) in RANGES {
            for code_point in first..=last {
                assert!(
                    is_control_or_format(scalar(code_point)),
                    "U+{code_point:04X}"
                );
            }
            if first > 0 {
                assert!(
                    !is_control_or_format(scalar(first - 1)),
                    "before U+{first:04X}"
                );
            }
            assert!(
                !is_control_or_format(scalar(last + 1)),
                "after U+{last:04X}"
            );
        }
    }

    #[test]
    fn replace_unsafe_replaces_every_range() {
        for (first, last) in RANGES {
            for code_point in [first, last] {
                let text = format!("a{}b", scalar(code_point));
                assert_eq!(replace_unsafe(&text), "a\u{FFFD}b", "U+{code_point:04X}");
            }
        }
    }

    #[test]
    fn escape_for_terminal_escapes_every_range() {
        for (first, last) in RANGES {
            for code_point in [first, last] {
                let text = format!("a{}b", scalar(code_point));
                let expected = format!("a\\u{{{code_point:x}}}b");
                assert_eq!(escape_for_terminal(&text), expected);
            }
        }
    }

    #[test]
    fn ordinary_text_is_left_alone() {
        let text = "Example Domain \u{2014} Mozilla Firefox \u{1F600}";
        assert_eq!(replace_unsafe(text), text);
        assert_eq!(escape_for_terminal(text), text);
    }

    #[test]
    fn display_name_rule() {
        assert!(is_display_name("Pixel 9"));
        assert!(is_display_name(&"a".repeat(64)));
        assert!(!is_display_name(""));
        assert!(!is_display_name(&"a".repeat(65)));
        // 22 scalars, 66 bytes: the limit counts bytes.
        assert!(!is_display_name(&"\u{2014}".repeat(22)));
        for (first, last) in RANGES {
            for code_point in [first, last] {
                assert!(!is_display_name(&format!("a{}", scalar(code_point))));
            }
        }
    }

    #[test]
    fn info_text_rule() {
        assert!(is_info_text("swayconnect-cli/0.1.0", 64));
        assert!(is_info_text("a b_c.d", 32));
        assert!(!is_info_text("", 32));
        assert!(!is_info_text(&"a".repeat(33), 32));
        assert!(!is_info_text("android\n", 32));
        assert!(!is_info_text("cli\u{1b}[2J", 64));
        assert!(!is_info_text("caf\u{e9}", 64));
    }
}
