//! Tiny 5×5 block-letter font for the key-press overlay.
//!
//! Each known glyph renders as a 5-row × 5-col `[&'static str; 5]`,
//! built out of `█` filled cells and ` ` empty cells. The font is
//! intentionally tiny — only the keys the overlay can produce — so the
//! lookup table fits in a screen of source.
//!
//! The overlay uses this font to render single-character labels at
//! roughly 5× the normal cell size. Multi-character labels (`PgDn`,
//! `Esc`, `Home`, etc.) bypass it and render as plain text inside the
//! same yellow billboard.
//!
//! Letters are case-insensitive — pressing `k` or `K` both render the
//! capital glyph.

/// Width and height of every glyph in the font.
pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 5;

type Glyph = [&'static str; GLYPH_H];

/// Look up the 5×5 glyph for a single character, if known. Returns
/// `None` for chars without a hand-drawn glyph; the caller falls back
/// to plain-text rendering.
pub fn glyph(c: char) -> Option<Glyph> {
    let upper = c.to_ascii_uppercase();
    Some(match upper {
        'A' => ["  █  ", " █ █ ", "█████", "█   █", "█   █"],
        'B' => ["████ ", "█   █", "████ ", "█   █", "████ "],
        'C' => [" ████", "█    ", "█    ", "█    ", " ████"],
        'D' => ["███  ", "█  █ ", "█   █", "█  █ ", "███  "],
        'E' => ["█████", "█    ", "███  ", "█    ", "█████"],
        'F' => ["█████", "█    ", "███  ", "█    ", "█    "],
        'G' => [" ████", "█    ", "█  ██", "█   █", " ████"],
        'H' => ["█   █", "█   █", "█████", "█   █", "█   █"],
        'I' => ["█████", "  █  ", "  █  ", "  █  ", "█████"],
        'J' => ["█████", "    █", "    █", "█   █", " ███ "],
        'K' => ["█   █", "█  █ ", "███  ", "█  █ ", "█   █"],
        'L' => ["█    ", "█    ", "█    ", "█    ", "█████"],
        'M' => ["█   █", "██ ██", "█ █ █", "█   █", "█   █"],
        'N' => ["█   █", "██  █", "█ █ █", "█  ██", "█   █"],
        'O' => [" ███ ", "█   █", "█   █", "█   █", " ███ "],
        'P' => ["████ ", "█   █", "████ ", "█    ", "█    "],
        'Q' => [" ███ ", "█   █", "█   █", "█  █ ", " ██ █"],
        'R' => ["████ ", "█   █", "████ ", "█  █ ", "█   █"],
        'S' => [" ████", "█    ", " ███ ", "    █", "████ "],
        'T' => ["█████", "  █  ", "  █  ", "  █  ", "  █  "],
        'U' => ["█   █", "█   █", "█   █", "█   █", " ███ "],
        'V' => ["█   █", "█   █", "█   █", " █ █ ", "  █  "],
        'W' => ["█   █", "█   █", "█ █ █", "██ ██", "█   █"],
        'X' => ["█   █", " █ █ ", "  █  ", " █ █ ", "█   █"],
        'Y' => ["█   █", " █ █ ", "  █  ", "  █  ", "  █  "],
        'Z' => ["█████", "   █ ", "  █  ", " █   ", "█████"],
        '0' => [" ███ ", "█  ██", "█ █ █", "██  █", " ███ "],
        '1' => ["  █  ", " ██  ", "  █  ", "  █  ", "█████"],
        '2' => [" ███ ", "█   █", "   █ ", "  █  ", "█████"],
        '3' => ["████ ", "    █", "  ██ ", "    █", "████ "],
        '4' => ["█   █", "█   █", "█████", "    █", "    █"],
        '5' => ["█████", "█    ", "████ ", "    █", "████ "],
        '6' => [" ████", "█    ", "████ ", "█   █", " ███ "],
        '7' => ["█████", "    █", "   █ ", "  █  ", " █   "],
        '8' => [" ███ ", "█   █", " ███ ", "█   █", " ███ "],
        '9' => [" ███ ", "█   █", " ████", "    █", "████ "],
        // Sort cyclers and miscellaneous one-glyph ASCII.
        '<' | ',' => ["    █", "  ██ ", "██   ", "  ██ ", "    █"],
        '>' | '.' => ["█    ", " ██  ", "   ██", " ██  ", "█    "],
        '/' => ["    █", "   █ ", "  █  ", " █   ", "█    "],
        '-' => ["     ", "     ", "█████", "     ", "     "],
        '+' => ["  █  ", "  █  ", "█████", "  █  ", "  █  "],
        '=' => ["     ", "█████", "     ", "█████", "     "],
        '?' => [" ███ ", "█   █", "   █ ", "  █  ", "  █  "],
        '!' => ["  █  ", "  █  ", "  █  ", "     ", "  █  "],
        ':' => ["     ", "  █  ", "     ", "  █  ", "     "],
        ' ' => ["     ", "     ", "     ", "     ", "     "],
        // Big arrow glyphs for the cursor keys.
        '▲' | '↑' => ["  █  ", " ███ ", "█████", "  █  ", "  █  "],
        '▼' | '↓' => ["  █  ", "  █  ", "█████", " ███ ", "  █  "],
        '◀' | '←' => ["  █  ", " ██  ", "█████", " ██  ", "  █  "],
        '▶' | '→' => ["  █  ", "  ██ ", "█████", "  ██ ", "  █  "],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_5x5() {
        for c in ('A'..='Z')
            .chain('0'..='9')
            .chain(['<', '>', '/', '-', '+', '=', '?', '!', ':', ' '])
            .chain(['▲', '▼', '◀', '▶'])
        {
            let g = glyph(c).unwrap_or_else(|| panic!("missing glyph for {c:?}"));
            for (i, row) in g.iter().enumerate() {
                assert_eq!(
                    row.chars().count(),
                    GLYPH_W,
                    "{c:?} row {i} has wrong width: {row:?}",
                );
            }
            assert_eq!(g.len(), GLYPH_H);
        }
    }

    #[test]
    fn lowercase_letters_map_to_uppercase() {
        assert_eq!(glyph('e'), glyph('E'));
        assert_eq!(glyph('q'), glyph('Q'));
        assert_eq!(glyph('z'), glyph('Z'));
    }

    #[test]
    fn arrows_map_to_chunky_block_form() {
        assert_eq!(glyph('↑'), glyph('▲'));
        assert_eq!(glyph('↓'), glyph('▼'));
        assert_eq!(glyph('←'), glyph('◀'));
        assert_eq!(glyph('→'), glyph('▶'));
    }

    #[test]
    fn unknown_chars_return_none() {
        assert!(glyph('@').is_none());
        assert!(glyph('λ').is_none());
    }
}
