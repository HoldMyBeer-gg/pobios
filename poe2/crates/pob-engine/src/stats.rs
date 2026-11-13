//! Named stats, and how they are formatted for display.
//!
//! Stat keys are PoB's own (`TotalDPS`, `AverageHit`, …) so they stay valid
//! across upstream updates. Display labels follow PoB's wording, since users
//! carry that vocabulary over.

/// How a value should be rendered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// Thousands-separated, one decimal: 1,829,571.8
    Number,
    /// Two decimals, no separator: 7.69
    Rate,
    /// Trailing percent sign: 14.4%
    Percent,
    /// Whole number: 120
    Integer,
}

impl Format {
    pub fn render(self, v: f64) -> String {
        match self {
            Self::Number => thousands(v, 1),
            Self::Rate => format!("{v:.2}"),
            Self::Percent => format!("{v:.2}%"),
            Self::Integer => thousands(v, 0),
        }
    }
}

/// Group a number with commas. PoB uses the locale separator; we use a comma
/// until locale handling lands, and the separator is the only thing that
/// should change when it does.
fn thousands(v: f64, decimals: usize) -> String {
    let neg = v < 0.0;
    let s = format!("{:.*}", decimals, v.abs());
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s.as_str(), None),
    };

    let mut grouped = String::with_capacity(int.len() + int.len() / 3);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }

    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&grouped);
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

/// When a stat is worth showing at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShowWhen {
    /// Always, if the engine produced a value.
    Always,
    /// Only when it differs from Total DPS.
    ///
    /// PoB's "inc. X" stats are *totals including* a damage source, not the
    /// damage that source deals. With no poison, "Total DPS inc. Poison"
    /// equals Total DPS exactly, and showing it implies poison is doing all
    /// that damage when it is doing none. PoB hides these rows for the same
    /// reason (`condFunc` in Modules/BuildDisplayStats.lua).
    DiffersFromTotalDps,
    /// Only when non-zero.
    ///
    /// For a damage source the build does not use, zero is not news — it is a
    /// row saying "you are not doing the thing you already know you are not
    /// doing". Distinct from a defensive stat, where zero armour is a fact
    /// worth seeing.
    NonZero,
}

/// A stat to display: PoB's key, our label, and how to format it.
#[derive(Clone, Copy, Debug)]
pub struct StatSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub format: Format,
    pub show_when: ShowWhen,
}

const fn spec(key: &'static str, label: &'static str, format: Format) -> StatSpec {
    StatSpec {
        key,
        label,
        format,
        show_when: ShowWhen::Always,
    }
}

/// A stat that is only meaningful when it differs from Total DPS.
const fn spec_if_differs(
    key: &'static str,
    label: &'static str,
    format: Format,
) -> StatSpec {
    StatSpec {
        key,
        label,
        format,
        show_when: ShowWhen::DiffersFromTotalDps,
    }
}

/// A stat only worth a row when the build actually produces it.
const fn spec_if_nonzero(
    key: &'static str,
    label: &'static str,
    format: Format,
) -> StatSpec {
    StatSpec {
        key,
        label,
        format,
        show_when: ShowWhen::NonZero,
    }
}

/// A titled run of stats, mirroring how PoB groups them.
pub struct StatGroup {
    pub title: &'static str,
    pub stats: &'static [StatSpec],
}

pub const OFFENCE: &[StatSpec] = &[
    spec("AverageHit", "Average Hit", Format::Number),
    spec("Speed", "Cast Rate", Format::Rate),
    spec("HitChance", "Hit Chance", Format::Percent),
    spec("CritChance", "Crit Chance", Format::Percent),
    spec("CritMultiplier", "Crit Multiplier", Format::Rate),
    spec("TotalDPS", "Total DPS", Format::Number),
    // PoB's own labels: these are totals *including* a source, not that
    // source's own damage.
    spec_if_differs("WithPoisonDPS", "Total DPS inc. Poison", Format::Number),
    spec_if_differs("WithIgniteDPS", "Total DPS inc. Ignite", Format::Number),
    spec_if_differs("WithBleedDPS", "Total DPS inc. Bleed", Format::Number),
    spec_if_differs("CombinedDPS", "Combined DPS", Format::Number),
    // A source's own damage stands alone, but only if the build deals any.
    spec_if_nonzero("PoisonDPS", "Single Poison DPS", Format::Number),
    spec_if_nonzero("TotalDotDPS", "Total DoT DPS", Format::Number),
];

pub const ATTRIBUTES: &[StatSpec] = &[
    spec("Str", "Strength", Format::Integer),
    spec("Dex", "Dexterity", Format::Integer),
    spec("Int", "Intelligence", Format::Integer),
];

pub const DEFENCE: &[StatSpec] = &[
    spec("Life", "Life", Format::Integer),
    spec("Mana", "Mana", Format::Integer),
    spec("EnergyShield", "Energy Shield", Format::Integer),
    spec("Armour", "Armour", Format::Integer),
    spec("Evasion", "Evasion", Format::Integer),
];

pub const GROUPS: &[StatGroup] = &[
    StatGroup {
        title: "Offence",
        stats: OFFENCE,
    },
    StatGroup {
        title: "Defence",
        stats: DEFENCE,
    },
    StatGroup {
        title: "Attributes",
        stats: ATTRIBUTES,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inc_stats_are_hidden_when_they_equal_total_dps() {
        // The bug this encodes: a build with no poison showed
        // "Poison DPS 60,843" — the same number as Total DPS — implying
        // poison was doing all the damage when it was doing none.
        let with_poison = OFFENCE
            .iter()
            .find(|s| s.key == "WithPoisonDPS")
            .expect("WithPoisonDPS is listed");
        assert_eq!(with_poison.show_when, ShowWhen::DiffersFromTotalDps);
        assert_eq!(with_poison.label, "Total DPS inc. Poison");
    }

    #[test]
    fn hiding_a_stat_is_not_the_same_as_having_no_value() {
        // The distinction this encodes, learned by getting it wrong: a stat
        // the engine did not compute deserves a dash, because "not
        // calculated" is information. A stat that is merely uninformative
        // must not occupy a row at all — rendering it as a dash just swaps a
        // misleading number for a misleading placeholder.
        //
        // ShowWhen carries the second case; the absence of a value carries
        // the first. Callers have to consult both.
        let inc = OFFENCE
            .iter()
            .find(|s| s.key == "WithPoisonDPS")
            .expect("listed");
        let plain = OFFENCE
            .iter()
            .find(|s| s.key == "AverageHit")
            .expect("listed");

        assert_eq!(inc.show_when, ShowWhen::DiffersFromTotalDps);
        assert_eq!(plain.show_when, ShowWhen::Always);
    }

    #[test]
    fn a_sources_own_damage_shows_only_when_there_is_some() {
        // PoisonDPS is poison's actual damage, so it is never misleading the
        // way an "inc. X" total is — but on a build with no poison it is
        // still a row saying nothing. Hidden when zero, shown when real.
        let poison = OFFENCE
            .iter()
            .find(|s| s.key == "PoisonDPS")
            .expect("PoisonDPS is listed");
        assert_eq!(poison.show_when, ShowWhen::NonZero);
    }

    #[test]
    fn defensive_stats_are_shown_even_at_zero() {
        // The counter-case that stops NonZero spreading everywhere: zero
        // armour or zero energy shield is a fact about the build worth
        // seeing, not an absent feature.
        for key in ["Armour", "EnergyShield", "Life"] {
            let spec = DEFENCE
                .iter()
                .find(|s| s.key == key)
                .unwrap_or_else(|| panic!("{key} is listed"));
            assert_eq!(spec.show_when, ShowWhen::Always, "{key}");
        }
    }

    #[test]
    fn thousands_groups_from_the_right() {
        assert_eq!(Format::Number.render(1_829_571.849), "1,829,571.8");
        assert_eq!(Format::Integer.render(120.0), "120");
        assert_eq!(Format::Integer.render(1234.0), "1,234");
        assert_eq!(Format::Number.render(7.69), "7.7");
    }

    #[test]
    fn negatives_keep_their_sign_outside_the_grouping() {
        assert_eq!(Format::Integer.render(-1234.0), "-1,234");
    }

    #[test]
    fn small_values_are_not_grouped() {
        assert_eq!(Format::Integer.render(0.0), "0");
        assert_eq!(Format::Integer.render(999.0), "999");
    }
}
