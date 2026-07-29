//! Issue #8: reconstructs a page's *visual* layout from per-character
//! geometry.
//!
//! ## Why this exists
//!
//! `pdf_sidecar` used to extract page text with pdfium's
//! `PdfPageText::all()`, which returns characters in content-stream order
//! and collapses every run of horizontal space to a single `' '`. Fed a PDF
//! whose columns are separated by wide gaps, it returns:
//!
//! ```text
//! 01/12/2025 SWIGGY ORDER BANGALORE 1,250.00 DR
//! ```
//!
//! The column structure is gone before any parser can see it. That single
//! fact broke statement parsing in two separate places:
//!
//! 1. **Rows.** `row_extractor::parse_icici_credit` separates its columns on
//!    `\s{2,}` — against `all()` output that can never match, so the parser
//!    was dead code. Worse, a real bank statement draws each table cell as
//!    its own text object, so `all()` emits one *cell* per line and no
//!    whole-row regex matches at all.
//! 2. **Header metadata.** `metadata_extractor`'s patterns want
//!    `label: value` adjacency (`total\s+amount\s+due[:\s]+([\d,]+)`). In a
//!    header laid out as a table the label and its value are separate cells,
//!    so `all()` emits them on separate lines and the pattern misses.
//!
//! Rebuilding the geometry here fixes both at once, and leaves all seven
//! bank parsers reading the shape they were written against.
//!
//! ## Approach
//!
//! Characters carry a bounding box. Group them into visual lines by vertical
//! overlap, order each line left-to-right, and re-insert horizontal space in
//! proportion to the gap actually present on the page. Whitespace characters
//! from the source are discarded — spacing is derived from geometry alone,
//! which is the whole point, and keeping them would double-count.
//!
//! Deliberately not handled: rotated text, right-to-left scripts, and
//! multi-column prose reflow. Indian bank statements are upright,
//! left-to-right, and tabular; the parsers downstream are line-oriented.

/// One character from the PDF, in a top-left origin coordinate space (y grows
/// downward — the caller flips pdfium's bottom-left origin before
/// constructing these).
///
/// The box spans the glyph's full laid-out advance, not its ink. That choice
/// is what makes spacing recoverable at all: advance boxes of consecutive
/// letters in a word abut exactly, so any positive gap between them is a real
/// space. See `SPACE_GAP_RATIO` for the measurements.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PositionedChar {
    pub text: String,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

/// A page with more characters than this is treated as adversarial and
/// truncated rather than reconstructed in full. A dense 20-page statement
/// runs ~3k characters per page; 200k is far beyond any legitimate one.
const MAX_CHARS_PER_PAGE: usize = 200_000;

/// Two characters belong to the same visual line when their vertical extents
/// overlap by at least this fraction of the shorter one's height. Generous
/// enough to keep a line together across mixed font sizes (a ₹ glyph next to
/// a larger numeral), tight enough not to swallow the line above.
const LINE_OVERLAP_RATIO: f32 = 0.5;

/// A horizontal gap wider than this multiple of the line's typical glyph
/// width becomes at least one space.
///
/// Measured on a real statement header (`TOTAL AMOUNT`, glyph width 5.40):
/// gaps between letters inside a word run from −0.59 to 0.00, the gap at the
/// word space is 1.51, and the gap at a column boundary is 74.56. The two
/// populations are cleanly separated, but *only just* at the word space —
/// 1.51/5.40 is 0.28, so a threshold of 0.28 sits exactly on the boundary and
/// welded "TOTAL AMOUNT" into "TOTALAMOUNT", which in turn defeats
/// `metadata_extractor`'s `total\s+amount\s+due` pattern. At 0.15 the
/// threshold sits between the populations with margin on both sides.
///
/// Tight ink extents were tried instead and are strictly worse: they measure
/// the gap after a comma (1.79) as larger than the gap at this word space,
/// so no threshold separates them and "1,250.00" splits into "1, 250.00".
const SPACE_GAP_RATIO: f32 = 0.15;

/// Never emit more than this many spaces for one gap. A statement's rightmost
/// column can sit half a page from the previous one; padding that faithfully
/// would produce enormous lines for no gain, since parsers only care that
/// *some* multi-space gap separates the columns.
const MAX_GAP_SPACES: usize = 24;

/// Rebuilds one page of text with its horizontal and vertical structure
/// intact. Returns lines top-to-bottom, joined by `\n`.
pub fn reconstruct_page(chars: &[PositionedChar]) -> String {
    let mut visible: Vec<&PositionedChar> = chars
        .iter()
        .take(MAX_CHARS_PER_PAGE)
        .filter(|c| !c.text.trim().is_empty())
        .collect();
    if visible.is_empty() {
        return String::new();
    }

    // Sort by vertical position first so line grouping below only ever has
    // to compare against the line currently being built.
    visible.sort_by(|a, b| {
        mid_y(a)
            .partial_cmp(&mid_y(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut lines: Vec<Vec<&PositionedChar>> = Vec::new();
    for ch in visible {
        match lines.last_mut() {
            Some(line) if shares_line(line[0], ch) => line.push(ch),
            _ => lines.push(vec![ch]),
        }
    }

    lines
        .iter_mut()
        .map(|line| {
            line.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
            render_line(line)
        })
        .filter(|line: &String| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn mid_y(c: &PositionedChar) -> f32 {
    (c.y0 + c.y1) / 2.0
}

/// Whether `candidate` sits on the same visual line as the characters already
/// collected.
///
/// Compared against the line's *first* character, not its last. Characters
/// arrive sorted by vertical midpoint, so chaining each test to the previous
/// character would let a run of glyphs each drifting a hair lower than the
/// one before accumulate without limit, welding a whole dense page into one
/// line. Anchoring on the first character bounds the line to one glyph height
/// no matter how many characters it collects. Mixed font sizes still group
/// correctly because the overlap is measured against the *shorter* of the two
/// extents, so a small glyph sitting inside a large one scores a full 1.0.
fn shares_line(anchor: &PositionedChar, candidate: &PositionedChar) -> bool {
    let overlap = (anchor.y1.min(candidate.y1) - anchor.y0.max(candidate.y0)).max(0.0);
    let shorter = (anchor.y1 - anchor.y0).min(candidate.y1 - candidate.y0);
    if shorter <= 0.0 {
        // Degenerate box (some fonts report zero-height glyphs) — fall back
        // to comparing midpoints against a small absolute tolerance.
        return (mid_y(anchor) - mid_y(candidate)).abs() < 2.0;
    }
    overlap / shorter >= LINE_OVERLAP_RATIO
}

/// Joins one line's characters, converting horizontal gaps back into spaces.
///
/// The space unit is the line's own median glyph width, not a document-wide
/// constant: a 14pt header and an 8pt table row have very different natural
/// letter spacing, and one global threshold either welds header words
/// together or shreds body text into spaced-out letters.
fn render_line(line: &[&PositionedChar]) -> String {
    let unit = median_width(line);
    let mut out = String::new();
    let mut prev_x1: Option<f32> = None;

    for ch in line {
        if let Some(prev) = prev_x1 {
            let gap = ch.x0 - prev;
            if gap > unit * SPACE_GAP_RATIO {
                let spaces = ((gap / unit).round() as usize).clamp(1, MAX_GAP_SPACES);
                out.push_str(&" ".repeat(spaces));
            }
        }
        out.push_str(&ch.text);
        prev_x1 = Some(ch.x1);
    }
    out.trim_end().to_string()
}

fn median_width(line: &[&PositionedChar]) -> f32 {
    let mut widths: Vec<f32> = line
        .iter()
        .map(|c| c.x1 - c.x0)
        .filter(|w| *w > 0.0)
        .collect();
    if widths.is_empty() {
        return 1.0;
    }
    widths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    widths[widths.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a run of characters on one baseline starting at `x`, each glyph
    /// `w` wide. Advance boxes abut exactly, as pdfium's loose boxes do
    /// within a word.
    fn cell(text: &str, x: f32, y: f32, w: f32) -> Vec<PositionedChar> {
        text.chars()
            .enumerate()
            .map(|(i, c)| PositionedChar {
                text: c.to_string(),
                x0: x + i as f32 * w,
                x1: x + (i as f32 + 1.0) * w,
                y0: y,
                y1: y + 10.0,
            })
            .collect()
    }

    /// The defect this module exists for: a statement row whose columns are
    /// separated by real page distance must come back with a multi-space gap
    /// the parsers can split on, not the single space `PdfPageText::all()`
    /// collapses it to.
    #[test]
    fn column_gaps_survive_as_multiple_spaces() {
        let mut chars = cell("01/12/2025", 50.0, 100.0, 6.0);
        chars.extend(cell("SWIGGY ORDER", 200.0, 100.0, 6.0));
        chars.extend(cell("1,250.00", 420.0, 100.0, 6.0));
        chars.extend(cell("DR", 520.0, 100.0, 6.0));

        let out = reconstruct_page(&chars);
        assert!(
            out.contains("01/12/2025  "),
            "column gap must widen to 2+ spaces, got {out:?}"
        );
        // `parse_icici_credit`'s column separator is literally `\s{2,}` — the
        // reason it could never match before this module existed.
        assert!(
            regex::Regex::new(r"SWIGGY ORDER\s{2,}1,250\.00")
                .unwrap()
                .is_match(&out),
            "amount column must be separable from the description, got {out:?}"
        );
    }

    /// Words inside a single cell are one space apart, not welded together
    /// and not blown apart into a padded gap.
    #[test]
    fn intra_cell_word_spacing_stays_one_space() {
        // "SWIGGY" occupies advances 100..136; a space glyph takes 136..142,
        // so the next word starts at 142 — exactly how a real font lays it out.
        let mut chars = cell("SWIGGY", 100.0, 50.0, 6.0);
        chars.extend(cell("ORDER", 142.0, 50.0, 6.0));
        let out = reconstruct_page(&chars);
        assert_eq!(out, "SWIGGY ORDER", "got {out:?}");
    }

    /// A table row drawn as separate cells at the same height is one line,
    /// even though the characters arrive interleaved rather than in reading
    /// order — this is exactly how pdfium hands over a real statement table.
    #[test]
    fn cells_at_the_same_height_form_one_line() {
        let mut chars = cell("1,250.00", 420.0, 100.0, 6.0);
        chars.extend(cell("01/12/2025", 50.0, 100.0, 6.0)); // out of reading order
        let out = reconstruct_page(&chars);
        assert_eq!(out.lines().count(), 1, "got {out:?}");
        assert!(out.starts_with("01/12/2025"), "got {out:?}");
    }

    /// Vertical structure is preserved and ordered top-to-bottom regardless
    /// of the order characters arrive in.
    #[test]
    fn lines_come_back_in_top_to_bottom_order() {
        let mut chars = cell("SECOND", 50.0, 200.0, 6.0);
        chars.extend(cell("FIRST", 50.0, 100.0, 6.0));
        chars.extend(cell("THIRD", 50.0, 300.0, 6.0));
        let out = reconstruct_page(&chars);
        assert_eq!(out, "FIRST\nSECOND\nTHIRD", "got {out:?}");
    }

    /// The header half of issue #8: a label and its value sitting side by
    /// side in a header table must land on one line, which is what
    /// `metadata_extractor`'s `label[:\s]+value` patterns require.
    #[test]
    fn header_label_and_value_land_on_one_line() {
        let mut chars = cell("Payment Due Date:", 60.0, 400.0, 5.0);
        chars.extend(cell("02-01-2026", 300.0, 400.0, 5.0));
        let out = reconstruct_page(&chars);
        assert!(
            regex::Regex::new(r"(?i)payment\s+due\s+date[:\s]+(\d{2}[/\-]\d{2}[/\-]\d{4})")
                .unwrap()
                .is_match(&out),
            "metadata_extractor's own due-date pattern must match, got {out:?}"
        );
    }

    /// A slight baseline drift across cells (common where a ₹ glyph or a
    /// differently-sized font sits in one column) must not split the row.
    #[test]
    fn small_baseline_drift_does_not_split_a_row() {
        let mut chars = cell("01/12/2025", 50.0, 100.0, 6.0);
        chars.extend(cell("1,250.00", 420.0, 102.0, 6.0));
        assert_eq!(reconstruct_page(&chars).lines().count(), 1);
    }

    /// Genuinely separate lines stay separate — the tolerance above must not
    /// be so loose that consecutive table rows merge into one.
    #[test]
    fn adjacent_rows_stay_separate() {
        let mut chars = cell("01/12/2025 FIRST ROW", 50.0, 100.0, 6.0);
        chars.extend(cell("02/12/2025 SECOND ROW", 50.0, 112.0, 6.0));
        assert_eq!(reconstruct_page(&chars).lines().count(), 2);
    }

    /// Regression, with the geometry measured off a real HDFC statement
    /// header: the word space in a large heading font is only 0.28 glyph
    /// widths, far narrower than the full-width space of body text. A
    /// threshold set for body text welds the heading into "TOTALAMOUNT",
    /// which then fails `metadata_extractor`'s `total\s+amount\s+due`.
    #[test]
    fn a_narrow_heading_word_space_still_separates() {
        let w = 5.4;
        let mut chars = cell("TOTAL", 128.0, 559.0, w);
        chars.extend(cell("AMOUNT", 128.0 + 5.0 * w + 1.51, 559.0, w));
        assert_eq!(reconstruct_page(&chars), "TOTAL AMOUNT");
    }

    /// Regression: a comma's own advance box abuts its neighbours, so an
    /// amount stays intact. (Measuring gaps from ink extents instead reports
    /// 1.79 here — wider than the real word space above — and splits this
    /// into "1, 250.00".)
    #[test]
    fn punctuation_does_not_split_an_amount() {
        let chars = cell("1,250.00", 420.0, 100.0, 6.0);
        assert_eq!(reconstruct_page(&chars), "1,250.00");
    }

    #[test]
    fn whitespace_only_input_yields_empty_string() {
        let chars = cell("   ", 50.0, 100.0, 6.0);
        assert_eq!(reconstruct_page(&chars), "");
        assert_eq!(reconstruct_page(&[]), "");
    }

    /// A gap spanning most of the page is capped rather than emitting
    /// hundreds of spaces.
    #[test]
    fn an_enormous_gap_is_capped() {
        let mut chars = cell("A", 10.0, 100.0, 6.0);
        chars.extend(cell("B", 5000.0, 100.0, 6.0));
        let out = reconstruct_page(&chars);
        assert_eq!(out.len(), 2 + MAX_GAP_SPACES, "got {out:?}");
    }
}
