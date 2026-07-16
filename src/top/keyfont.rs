//! Compact 3×5 block-letter font for the key-press overlay.
//!
//! Each known glyph renders as a 3-row × 5-col `[&'static str; 3]`
//! built from `█` filled cells and `' '` empty cells. Uppercase only —
//! pressing `k` or `K` both render the capital form. The width and
//! height are the smallest combination that keeps every letter legible
//! and lets multi-character labels (`PGDN`, `ESC`, `HOME`, …) render at
//! the same scale as single-character labels.
//!
//! When a label has no hand-drawn glyph for a character, the renderer
//! falls back to centred plain text inside the same yellow billboard.

/// Width and height of every glyph in the font. `GLYPH_W` is part of
/// the public surface so future callers can compute label widths
/// without reading them off `render_label`'s output; `GLYPH_H` is used
/// by both the test suite and `render_label`.
#[allow(dead_code)]
pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 3;

/// Number of blank cells between two glyphs in a multi-character label.
pub const GLYPH_GAP: usize = 1;

type Glyph = [&'static str; GLYPH_H];

/// Look up the 3×5 glyph for a single character, if known. Returns
/// `None` for chars without a hand-drawn glyph; the caller falls back
/// to plain-text rendering.
///
/// Some glyphs are visually identical at this resolution (e.g. `I` and
/// `Z` both reduce to a thin vertical bar in 3×5 cells). Clippy's
/// `match_same_arms` would have us collapse them into one arm, but
/// keeping them distinct keeps the table readable for font tweaks.
#[allow(clippy::match_same_arms)]
pub fn glyph(c: char) -> Option<Glyph> {
    let upper = c.to_ascii_uppercase();
    Some(match upper {
        'A' => [" ███ ", "█████", "█   █"],
        'B' => ["████ ", "█████", "████ "],
        'C' => [" ████", "█    ", " ████"],
        'D' => ["███  ", "█  █ ", "███  "],
        'E' => ["█████", "███  ", "█████"],
        'F' => ["█████", "███  ", "█    "],
        'G' => [" ████", "█  ██", " ████"],
        'H' => ["█   █", "█████", "█   █"],
        'I' => ["  █  ", "  █  ", "  █  "],
        'J' => ["█████", "    █", " ███ "],
        'K' => ["█  █ ", "███  ", "█  █ "],
        'L' => ["█    ", "█    ", "█████"],
        'M' => ["█▄ ▄█", "█ █ █", "█   █"],
        'N' => ["██  █", "█ █ █", "█  ██"],
        'O' => [" ███ ", "█   █", " ███ "],
        'P' => ["████ ", "████ ", "█    "],
        'Q' => [" ███ ", "█   █", " ██ █"],
        'R' => ["████ ", "███  ", "█  █ "],
        'S' => [" ████", " ███ ", "████ "],
        'T' => ["█████", "  █  ", "  █  "],
        'U' => ["█   █", "█   █", " ███ "],
        'V' => ["█   █", " █ █ ", "  █  "],
        'W' => ["█   █", "█ █ █", " █ █ "],
        'X' => ["█   █", "  █  ", "█   █"],
        'Y' => ["█   █", "  █  ", "  █  "],
        'Z' => ["█████", "  █  ", "█████"],
        '0' => [" ███ ", "█ █ █", " ███ "],
        '1' => ["  █  ", "  █  ", "█████"],
        '2' => ["████ ", "  ██ ", "█████"],
        '3' => ["████ ", "  ██ ", "████ "],
        '4' => ["█   █", "█████", "    █"],
        '5' => ["█████", "████ ", "████ "],
        '6' => [" ████", "████ ", " ███ "],
        '7' => ["█████", "   █ ", " █   "],
        '8' => [" ███ ", " ███ ", " ███ "],
        '9' => [" ███ ", " ████", "████ "],
        // Sort cyclers + miscellaneous one-glyph ASCII.
        '<' | ',' => ["    █", "███  ", "    █"],
        '>' | '.' => ["█    ", "  ███", "█    "],
        '/' => ["    █", "  █  ", "█    "],
        '-' => ["     ", "█████", "     "],
        '+' => ["  █  ", "█████", "  █  "],
        '=' => ["█████", "     ", "█████"],
        '?' => ["████ ", "  ██ ", "  █  "],
        '!' => ["  █  ", "  █  ", "  █  "],
        ':' => ["  █  ", "     ", "  █  "],
        ' ' => ["     ", "     ", "     "],
        // Filled directional triangles — each direction gets a
        // distinct shape. Up/down are pointing along the rows; left
        // and right are pointing along the columns.
        '▲' | '↑' => ["  █  ", " ███ ", "█████"],
        '▼' | '↓' => ["█████", " ███ ", "  █  "],
        '◀' | '←' => ["  ███", "█████", "  ███"],
        '▶' | '→' => ["███  ", "█████", "███  "],
        _ => return None,
    })
}

/// Render a label as a multi-glyph block, joining each character's
/// glyph with `GLYPH_GAP` blank columns. Returns `None` when *any*
/// character in the label has no hand-drawn glyph; the caller falls
/// back to centred plain text.
pub fn render_label(label: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = label.chars().collect();
    let mut glyphs = Vec::with_capacity(chars.len());
    for c in &chars {
        glyphs.push(glyph(*c)?);
    }

    let gap: String = " ".repeat(GLYPH_GAP);
    let mut rows = Vec::with_capacity(GLYPH_H);
    for r in 0..GLYPH_H {
        let mut line = String::new();
        for (i, g) in glyphs.iter().enumerate() {
            if i > 0 {
                line.push_str(&gap);
            }
            line.push_str(g[r]);
        }
        rows.push(line);
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_3x5() {
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
    fn unknown_chars_return_none() {
        assert!(glyph('@').is_none());
        assert!(glyph('λ').is_none());
    }

    #[test]
    fn render_label_concatenates_glyphs_with_gap() {
        let rows = render_label("AB").expect("known label");
        assert_eq!(rows.len(), GLYPH_H);
        // Each row: 5 glyph cols + 1 gap col + 5 glyph cols = 11.
        for row in &rows {
            assert_eq!(row.chars().count(), GLYPH_W * 2 + GLYPH_GAP);
        }
        // Should contain the top row of A followed by gap then top row of B.
        assert!(rows[0].starts_with(" ███ "));
        assert!(rows[0].ends_with("████ "));
    }

    #[test]
    fn render_label_returns_none_for_unknown_char() {
        // λ has no glyph → entire label fails (caller falls back to text).
        assert!(render_label("Aλ").is_none());
    }
}
