//! The Calcs tab's sections, rows, and cells.
//!
//! PoB describes these declaratively in `Modules/CalcSections.lua`, but the
//! cell *values* come from format strings like `{0:mod:1,2}` that its own
//! `formatCalcStr` resolves against the live calc environment.
//!
//! We call that function rather than reimplementing the interpreter. It is a
//! plain global in `Modules/CalcFormat.lua`, it already produces exactly what
//! PoB displays (separators, percent signs and all), and it keeps working when
//! upstream changes the format language.

/// One value cell in a section row.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Cell {
    /// Already formatted by PoB. Empty when the cell has no value in this
    /// build (PoB leaves those blank rather than showing a zero).
    pub value: String,
    /// The cell has a breakdown or modifier list behind it, which PoB shows
    /// on click.
    pub has_breakdown: bool,
}

/// One section of PoB's stat breakdown for a cell: lines of text, or a
/// table with named columns.
#[derive(Clone, Debug, PartialEq)]
pub struct BreakdownPart {
    pub label: Option<String>,
    pub footer: Option<String>,
    /// Text lines; empty for a table.
    pub lines: Vec<String>,
    /// Column labels; empty for text.
    pub columns: Vec<String>,
    /// Rows of cells, one per column.
    pub rows: Vec<Vec<String>>,
}

/// The header inputs of PoB's Calcs tab. Its skill selection is separate
/// from the sidebar's: `skill_number` picks the socket group, and the part,
/// stages, mines and minion choices are the `...Calcs` variants.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CalcsInputs {
    pub groups: Vec<String>,
    /// 1-based socket group index (`input.skill_number`).
    pub group_index: usize,
    pub controls: MainSkillControls,
    /// UNBUFFED, BUFFED, COMBAT or EFFECTIVE.
    pub buff_mode: String,
    pub has_minion: bool,
    pub show_minion: bool,
}

pub use crate::skills::MainSkillControls;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Row label, e.g. "Added Min". Empty for the header row that names the
    /// damage-type columns.
    pub label: String,
    pub cells: Vec<Cell>,
    /// True for the header row, whose cells are column titles rather than
    /// values.
    pub is_header: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Section {
    /// PoB's own id, e.g. "HitDamage".
    pub id: String,
    /// Human title, e.g. "Skill Hit Damage".
    pub label: String,
    pub rows: Vec<Row>,
    /// PoB's column: 1 offence, 2 resources, 3 defence.
    pub column: u8,
    /// PoB's colour for the section frame, as `#rrggbb`.
    pub colour: String,
    /// The header value PoB draws after the title, e.g. "35,895.7 DPS".
    pub extra: String,
    pub collapsed_default: bool,
    /// Where the section lives in PoB's list, for breakdown lookups.
    pub section_index: usize,
    pub sub_index: usize,
}

impl Section {
    /// True when nothing in this section has a value, which happens often:
    /// a spell build has no attack sections, a life build no ward ones. PoB
    /// hides those rather than showing a table of blanks.
    pub fn is_empty(&self) -> bool {
        self.rows
            .iter()
            .filter(|r| !r.is_header)
            .all(|r| r.cells.iter().all(|c| c.value.trim().is_empty()))
    }

    /// Rows that actually carry a value, for a compact view.
    pub fn populated_rows(&self) -> impl Iterator<Item = &Row> {
        self.rows
            .iter()
            .filter(|r| r.is_header || r.cells.iter().any(|c| !c.value.trim().is_empty()))
    }
}

/// Strip PoB's `^xRRGGBB` and `^N` colour escapes from a display string.
///
/// Column headers arrive as `"^xADAA47Lightning:"`. We re-colour from our own
/// tokens, so the escape is noise that would otherwise render literally.
pub fn strip_color_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // ^xRRGGBB
            Some('x') | Some('X') => {
                chars.next();
                for _ in 0..6 {
                    if chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                        chars.next();
                    }
                }
            }
            // ^0..^9
            Some(d) if d.is_ascii_digit() => {
                chars.next();
            }
            // A bare caret is literal.
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_escapes_are_stripped_from_labels() {
        assert_eq!(strip_color_escapes("^xADAA47Lightning:"), "Lightning:");
        assert_eq!(strip_color_escapes("^7Total"), "Total");
        assert_eq!(strip_color_escapes("plain"), "plain");
    }

    #[test]
    fn a_bare_caret_survives() {
        // Not every caret is an escape; dropping one would corrupt the text.
        assert_eq!(strip_color_escapes("2 ^ 3"), "2 ^ 3");
    }

    #[test]
    fn an_empty_section_is_recognised() {
        // Sections with no values are hidden rather than shown as blanks: a
        // spell build has no attack rows, and a wall of empty tables would
        // bury the sections that do matter.
        let empty = Section {
            id: "Impale".into(),
            label: "Impale".into(),
            rows: vec![Row {
                label: "Chance".into(),
                cells: vec![Cell { value: String::new(), has_breakdown: false }],
                is_header: false,
            }],
            ..Default::default()
        };
        assert!(empty.is_empty());

        let populated = Section {
            id: "Life".into(),
            label: "Life".into(),
            rows: vec![Row {
                label: "Total".into(),
                cells: vec![Cell { value: "1,376".into(), has_breakdown: false }],
                is_header: false,
            }],
            ..Default::default()
        };
        assert!(!populated.is_empty());
    }

    #[test]
    fn a_header_only_section_counts_as_empty() {
        // A header names columns but says nothing about the build, so a
        // section carrying only one is still nothing to show.
        let s = Section {
            id: "HitDamage".into(),
            label: "Skill Hit Damage".into(),
            rows: vec![Row {
                label: String::new(),
                cells: vec![Cell { value: "Physical:".into(), has_breakdown: false }],
                is_header: true,
            }],
            ..Default::default()
        };
        assert!(s.is_empty());
    }
}
