//! Reconstructs page structure from positioned characters.
//!
//! A PDF stores glyphs with coordinates, not rows and columns, so a statement
//! table arrives as a cloud of positioned characters. This groups them back into
//! lines and columns by position, which is what makes row extraction possible at
//! all -- reading the raw text stream in order would interleave columns.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PositionedChar {
    pub text: String,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

const MAX_CHARS_PER_PAGE: usize = 200_000;

const LINE_OVERLAP_RATIO: f32 = 0.5;

const SPACE_GAP_RATIO: f32 = 0.15;

const MAX_GAP_SPACES: usize = 24;

/// Reconstructs readable text from positioned characters.
///
/// A PDF stores glyphs with coordinates, not rows and columns, so a statement
/// table arrives as a cloud of positioned characters. Reading the raw text stream
/// in order would interleave columns into nonsense; grouping by vertical position
/// and then inferring gaps is what recovers the table.
pub fn reconstruct_page(chars: &[PositionedChar]) -> String {
    let mut visible: Vec<&PositionedChar> = chars
        .iter()
        .take(MAX_CHARS_PER_PAGE)
        .filter(|c| !c.text.trim().is_empty())
        .collect();
    if visible.is_empty() {
        return String::new();
    }

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

/// Vertical centre of a character's bounding box.
fn mid_y(c: &PositionedChar) -> f32 {
    (c.y0 + c.y1) / 2.0
}

/// Whether two characters belong to the same visual line.
///
/// Compares the proportion of vertical overlap rather than exact coordinates,
/// since characters on one line differ in height -- a comma and a capital do not
/// share a baseline box. Zero-height glyphs fall back to a midpoint comparison,
/// as an overlap ratio is undefined for them.
fn shares_line(anchor: &PositionedChar, candidate: &PositionedChar) -> bool {
    let overlap = (anchor.y1.min(candidate.y1) - anchor.y0.max(candidate.y0)).max(0.0);
    let shorter = (anchor.y1 - anchor.y0).min(candidate.y1 - candidate.y0);
    if shorter <= 0.0 {
        return (mid_y(anchor) - mid_y(candidate)).abs() < 2.0;
    }
    overlap / shorter >= LINE_OVERLAP_RATIO
}

/// Renders one line, inserting spaces where the glyphs leave gaps.
///
/// Column separation in a PDF is horizontal distance, not space characters, so
/// gaps are measured against the median glyph width and converted into spaces.
/// The count is clamped, or a wide table would produce enormous runs of spaces.
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

/// Median glyph width on a line, used as the spacing unit.
///
/// The median rather than the mean, so one unusually wide glyph cannot distort
/// the gap calculation for the whole line.
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
        assert!(
            regex::Regex::new(r"SWIGGY ORDER\s{2,}1,250\.00")
                .unwrap()
                .is_match(&out),
            "amount column must be separable from the description, got {out:?}"
        );
    }

    #[test]
    fn intra_cell_word_spacing_stays_one_space() {
        let mut chars = cell("SWIGGY", 100.0, 50.0, 6.0);
        chars.extend(cell("ORDER", 142.0, 50.0, 6.0));
        let out = reconstruct_page(&chars);
        assert_eq!(out, "SWIGGY ORDER", "got {out:?}");
    }

    #[test]
    fn cells_at_the_same_height_form_one_line() {
        let mut chars = cell("1,250.00", 420.0, 100.0, 6.0);
        chars.extend(cell("01/12/2025", 50.0, 100.0, 6.0));
        let out = reconstruct_page(&chars);
        assert_eq!(out.lines().count(), 1, "got {out:?}");
        assert!(out.starts_with("01/12/2025"), "got {out:?}");
    }

    #[test]
    fn lines_come_back_in_top_to_bottom_order() {
        let mut chars = cell("SECOND", 50.0, 200.0, 6.0);
        chars.extend(cell("FIRST", 50.0, 100.0, 6.0));
        chars.extend(cell("THIRD", 50.0, 300.0, 6.0));
        let out = reconstruct_page(&chars);
        assert_eq!(out, "FIRST\nSECOND\nTHIRD", "got {out:?}");
    }

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

    #[test]
    fn small_baseline_drift_does_not_split_a_row() {
        let mut chars = cell("01/12/2025", 50.0, 100.0, 6.0);
        chars.extend(cell("1,250.00", 420.0, 102.0, 6.0));
        assert_eq!(reconstruct_page(&chars).lines().count(), 1);
    }

    #[test]
    fn adjacent_rows_stay_separate() {
        let mut chars = cell("01/12/2025 FIRST ROW", 50.0, 100.0, 6.0);
        chars.extend(cell("02/12/2025 SECOND ROW", 50.0, 112.0, 6.0));
        assert_eq!(reconstruct_page(&chars).lines().count(), 2);
    }

    #[test]
    fn a_narrow_heading_word_space_still_separates() {
        let w = 5.4;
        let mut chars = cell("TOTAL", 128.0, 559.0, w);
        chars.extend(cell("AMOUNT", 128.0 + 5.0 * w + 1.51, 559.0, w));
        assert_eq!(reconstruct_page(&chars), "TOTAL AMOUNT");
    }

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

    #[test]
    fn an_enormous_gap_is_capped() {
        let mut chars = cell("A", 10.0, 100.0, 6.0);
        chars.extend(cell("B", 5000.0, 100.0, 6.0));
        let out = reconstruct_page(&chars);
        assert_eq!(out.len(), 2 + MAX_GAP_SPACES, "got {out:?}");
    }
}
