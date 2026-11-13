//! Configuration options: the Config tab's inputs.
//!
//! Definitions come from `Modules/ConfigOptions.lua`, which returns a flat
//! list where `{ section = ... }` entries mark boundaries and `{ var = ... }`
//! entries are options. Current values live in `build.configTab.input`.

/// What kind of control an option needs.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionKind {
    /// A toggle. PoB stores these as booleans.
    Check,
    /// A number. `allow_zero` distinguishes PoB's `count` (blank means unset)
    /// from `countAllowZero` (zero is a real value).
    Number { allow_zero: bool, float: bool },
    /// A choice. Values are PoB's internal keys; labels are what to show.
    List { options: Vec<(String, String)> },
}

/// The current value of an option, mirroring what PoB stores in `input`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
    /// The option is unset, which for PoB means "use the default".
    Unset,
}

impl Value {
    pub fn as_bool(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            // PoB stores some numeric inputs as strings.
            Self::Text(t) => t.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigOption {
    /// PoB's variable name, which is also the key in `input`.
    pub var: String,
    pub label: String,
    pub section: String,
    pub kind: OptionKind,
    pub value: Value,
    /// PoB's own default for a list option, when it is not the first entry.
    ///
    /// PoB writes this into `input` at load, so clearing the key is not the
    /// inverse of setting one: restoring means writing the default back.
    pub default: Option<String>,
    /// PoB would show this option for the current build: its `if...`
    /// conditions (skill flags, allocated nodes, other options) hold.
    pub relevant: bool,
    /// PoB's help text for the option, when it has one.
    pub tooltip: Option<String>,
}

/// One block of custom modifiers on the Config tab.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomModBlock {
    pub title: String,
    pub enabled: bool,
    /// One modifier per line, as PoB parses them.
    pub text: String,
}

impl ConfigOption {
    /// True when this option is set to something other than its default,
    /// which is worth surfacing: a build's behaviour often hinges on a
    /// handful of toggles buried in a list of hundreds.
    pub fn is_set(&self) -> bool {
        match &self.value {
            Value::Unset => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0,
            Value::Text(t) => !t.is_empty() && t != "None",
        }
    }

    /// Case-insensitive match against label or variable name, for the search
    /// box. Searching the var name too means a user who knows PoB's internals
    /// can find an option by the name they know it by.
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        self.label.to_lowercase().contains(&q) || self.var.to_lowercase().contains(&q)
    }
}

/// Strip PoB's colour escapes and trailing punctuation from a label.
///
/// Labels arrive as `"Enemy Skill ^xD02090Chaos Damage:"`. We re-colour from
/// our own tokens, and the trailing colon is PoB's layout, not part of the
/// name.
pub fn clean_label(s: &str) -> String {
    let stripped = crate::calcs::strip_color_escapes(s);
    stripped.trim().trim_end_matches(':').trim().to_owned()
}

/// PoB stuffs `\n` into tooltip prose to wrap it in its own renderer, often
/// mid-sentence ("active or\nan Iron Flask"). Collapse every newline to a
/// space and let the UI wrap; also strip colour escapes.
pub fn clean_tooltip(s: &str) -> String {
    let stripped = crate::calcs::strip_color_escapes(s);
    let mut out = String::with_capacity(stripped.len());
    let mut prev_space = false;
    for ch in stripped.chars() {
        let c = if ch == '\n' || ch == '\r' || ch == '\t' { ' ' } else { ch };
        if c == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(var: &str, label: &str, value: Value) -> ConfigOption {
        ConfigOption {
            var: var.into(),
            label: label.into(),
            section: "General".into(),
            kind: OptionKind::Check,
            value,
            default: None,
            relevant: true,
            tooltip: None,
        }
    }

    #[test]
    fn labels_lose_colour_escapes_and_trailing_colons() {
        assert_eq!(
            clean_label("Enemy Skill ^xD02090Chaos Damage:"),
            "Enemy Skill Chaos Damage"
        );
        assert_eq!(clean_label("Is the enemy a Boss?"), "Is the enemy a Boss?");
    }

    #[test]
    fn search_matches_label_or_variable_name() {
        let o = opt("enemyIsBoss", "Is the enemy a Boss?", Value::Unset);
        assert!(o.matches("boss"));
        assert!(o.matches("BOSS"));
        // Someone who knows PoB's internals can search the var name.
        assert!(o.matches("enemyIs"));
        assert!(!o.matches("shocked"));
        // An empty query matches everything rather than nothing.
        assert!(o.matches(""));
    }

    #[test]
    fn an_unset_option_is_not_reported_as_set() {
        assert!(!opt("a", "A", Value::Unset).is_set());
        assert!(!opt("a", "A", Value::Bool(false)).is_set());
        assert!(opt("a", "A", Value::Bool(true)).is_set());
    }

    #[test]
    fn none_counts_as_unset_for_list_options() {
        // PoB uses the literal "None" for an unselected list entry, so
        // treating it as set would mark most of the tab as configured.
        assert!(!opt("a", "A", Value::Text("None".into())).is_set());
        assert!(opt("a", "A", Value::Text("Boss".into())).is_set());
    }

    #[test]
    fn numeric_values_survive_pobs_string_storage() {
        // PoB stores some numeric inputs as strings; reading only the number
        // variant would show those fields as empty.
        assert_eq!(Value::Text("84".into()).as_number(), Some(84.0));
        assert_eq!(Value::Number(84.0).as_number(), Some(84.0));
        assert_eq!(Value::Text("".into()).as_number(), None);
    }
}
