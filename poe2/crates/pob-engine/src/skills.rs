//! Socket groups and gems, read from PoB's live skill state.

/// Socket colour. PoB stores these as single letters on each gem.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SocketColor {
    /// Strength / red
    Red,
    /// Dexterity / green
    Green,
    /// Intelligence / blue
    Blue,
    /// White sockets accept any gem.
    White,
}

impl SocketColor {
    /// Parse PoB's gem colour.
    ///
    /// PoB stores this as one of its own escape codes (`"^xE05030"`), not as
    /// a letter — the hex values are the `colorCodes` entries from
    /// `Data/Global.lua`: MARAUDER red, RANGER green, WITCH blue. Letters are
    /// accepted too, since build XML and our own tests use them.
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();

        if let Some(hex) = t.strip_prefix("^x") {
            return match hex.to_ascii_uppercase().as_str() {
                "E05030" => Some(Self::Red),
                "70FF70" => Some(Self::Green),
                "7070FF" => Some(Self::Blue),
                // PoB paints white/any sockets with the normal item colour.
                "C8C8C8" | "FFFFFF" => Some(Self::White),
                _ => None,
            };
        }

        match t.to_ascii_uppercase().as_str() {
            "R" => Some(Self::Red),
            "G" => Some(Self::Green),
            "B" => Some(Self::Blue),
            "W" => Some(Self::White),
            _ => None,
        }
    }

    /// The letter shown in the UI. Meaning is carried by this letter, never by
    /// hue alone, so socket colours stay readable under any colourblind mode.
    pub fn letter(self) -> &'static str {
        match self {
            Self::Red => "R",
            Self::Green => "G",
            Self::Blue => "B",
            Self::White => "W",
        }
    }

    /// PoB's own colour for this socket (`Data/Global.lua` colorCodes:
    /// R = MARAUDER, G = RANGER, B = WITCH).
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Red => (0xE0, 0x50, 0x30),
            Self::Green => (0x70, 0xFF, 0x70),
            Self::Blue => (0x70, 0x70, 0xFF),
            Self::White => (0xD8, 0xDE, 0xE6),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gem {
    pub name: String,
    /// PoB's skill id. Supports are identified by this starting with
    /// `Support`, never by the display name.
    pub skill_id: String,
    pub level: i64,
    pub quality: i64,
    /// Quality after item/socket bonuses (Dialla's, +quality-of-socketed
    /// mods). Equals `quality` when nothing boosts it.
    pub effective_quality: i64,
    pub enabled: bool,
    pub count: i64,
    pub color: Option<SocketColor>,
}

impl Gem {
    pub fn is_support(&self) -> bool {
        self.skill_id.starts_with("Support")
    }
}

/// One gem the picker can offer.
///
/// Carries the id because that is what identifies a gem to PoB — the display
/// name is what the user reads, but two gems can share a name across
/// versions, and the id is what `ProcessSocketGroup` resolves from.
#[derive(Clone, Debug, PartialEq)]
pub struct GemChoice {
    pub id: String,
    pub name: String,
    /// Socket colour, which PoB uses to tint the name in its dropdown.
    pub color: Option<SocketColor>,
    pub support: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SocketGroup {
    /// What PoB shows in its group list.
    pub label: String,
    /// Equipment slot, absent for groups granted by an item rather than
    /// socketed (those carry `source` instead).
    pub slot: Option<String>,
    pub enabled: bool,
    pub include_in_full_dps: bool,
    /// The equipped item's socket colours (e.g. "R-B-B-B-B-B"), empty when
    /// the group is granted or the slot has no socketed item.
    pub item_sockets: String,
    /// Set when the group comes from an item or effect rather than from
    /// sockets. Such a group cannot be edited like a socketed one.
    ///
    /// Two shapes occur: a global effect with no slot (`"Explode"`), and one
    /// granted by a specific equipped item, which carries that item's slot
    /// (`"Item:16:Asenath's Gentle Touch, Silk Gloves"`). So a slot being
    /// present does NOT mean the group is socketed; check `source` for that.
    pub source: Option<String>,
    pub gems: Vec<Gem>,
}

impl SocketGroup {
    /// Socket colours in order, for the R-B-B-G chips under the group name.
    pub fn socket_colors(&self) -> Vec<SocketColor> {
        self.gems.iter().filter_map(|g| g.color).collect()
    }

    /// Active (non-support) gems, which is what names the group.
    pub fn active_gems(&self) -> impl Iterator<Item = &Gem> {
        self.gems.iter().filter(|g| !g.is_support())
    }

    /// True when this group comes from an item or effect rather than sockets,
    /// and so cannot have its gems edited.
    pub fn is_granted(&self) -> bool {
        self.source.is_some()
    }

    /// The item a granted group came from, without PoB's `Item:<n>:` prefix.
    pub fn granted_by(&self) -> Option<&str> {
        let source = self.source.as_deref()?;
        Some(match source.strip_prefix("Item:") {
            // "Item:16:Asenath's Gentle Touch, Silk Gloves" → the name after
            // the index.
            Some(rest) => rest.split_once(':').map_or(rest, |(_, name)| name),
            None => source,
        })
    }

    /// PoB labels a group by its active gems when no explicit label is set.
    pub fn display_label(&self) -> String {
        if !self.label.is_empty() {
            return self.label.clone();
        }
        let names: Vec<&str> = self
            .active_gems()
            .filter(|g| !g.name.is_empty())
            .map(|g| g.name.as_str())
            .collect();
        if !names.is_empty() {
            return names.join(", ");
        }
        // A granted group often has no named gems; fall back to what granted
        // it rather than showing an empty row.
        match self.granted_by() {
            Some(source) => source.to_owned(),
            None => "(empty)".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gem(name: &str, skill_id: &str) -> Gem {
        Gem {
            name: name.to_owned(),
            skill_id: skill_id.to_owned(),
            level: 20,
            quality: 0,
            effective_quality: 0,
            enabled: true,
            count: 1,
            color: Some(SocketColor::Blue),
        }
    }

    #[test]
    fn supports_are_detected_by_skill_id_not_display_name() {
        // A gem can be *named* like a support without being one, and vice
        // versa; PoB's skillId prefix is the only reliable signal.
        assert!(gem("Withering Touch", "SupportWitheringTouch").is_support());
        assert!(!gem("Support Nothing", "Cremation").is_support());
    }

    #[test]
    fn a_group_is_labelled_by_its_active_gems() {
        let group = SocketGroup {
            label: String::new(),
            slot: Some("Gloves".into()),
            enabled: true,
            include_in_full_dps: false,
            item_sockets: String::new(),
            source: None,
            gems: vec![
                gem("Cremation", "Cremation"),
                gem("Elemental Focus", "SupportElementalFocus"),
                gem("Desecrate", "Desecrate"),
            ],
        };
        assert_eq!(group.display_label(), "Cremation, Desecrate");
    }

    #[test]
    fn an_explicit_label_wins_over_the_gem_names() {
        let group = SocketGroup {
            label: "My CWDT setup".into(),
            slot: None,
            enabled: true,
            include_in_full_dps: false,
            item_sockets: String::new(),
            source: None,
            gems: vec![gem("Cremation", "Cremation")],
        };
        assert_eq!(group.display_label(), "My CWDT setup");
    }

    #[test]
    fn a_group_with_only_supports_is_not_left_nameless() {
        let group = SocketGroup {
            label: String::new(),
            slot: None,
            enabled: true,
            include_in_full_dps: false,
            item_sockets: String::new(),
            source: None,
            gems: vec![gem("Withering Touch", "SupportWitheringTouch")],
        };
        assert_eq!(group.display_label(), "(empty)");
    }

    #[test]
    fn a_granted_group_names_itself_after_its_source() {
        let group = SocketGroup {
            label: String::new(),
            slot: Some("Gloves".into()),
            enabled: true,
            include_in_full_dps: false,
            item_sockets: String::new(),
            source: Some("Item:16:Asenath's Gentle Touch, Silk Gloves".into()),
            gems: vec![],
        };
        assert!(group.is_granted());
        // PoB's "Item:<index>:" prefix is bookkeeping, not something to show.
        assert_eq!(
            group.granted_by(),
            Some("Asenath's Gentle Touch, Silk Gloves")
        );
        assert_eq!(
            group.display_label(),
            "Asenath's Gentle Touch, Silk Gloves"
        );
    }

    #[test]
    fn a_granted_group_can_still_carry_a_slot() {
        // Item-granted groups report the slot of the item that granted them,
        // so a slot does not imply the group is socketed and editable.
        let group = SocketGroup {
            label: String::new(),
            slot: Some("Gloves".into()),
            enabled: true,
            include_in_full_dps: false,
            item_sockets: String::new(),
            source: Some("Item:16:Asenath's".into()),
            gems: vec![],
        };
        assert!(group.slot.is_some() && group.is_granted());
    }

    #[test]
    fn a_global_granted_effect_has_no_slot() {
        let group = SocketGroup {
            label: "On Kill Monster Explosion".into(),
            slot: None,
            enabled: true,
            include_in_full_dps: false,
            item_sockets: String::new(),
            source: Some("Explode".into()),
            gems: vec![],
        };
        assert!(group.is_granted());
        assert_eq!(group.granted_by(), Some("Explode"));
    }

    #[test]
    fn pobs_escape_code_colours_are_understood() {
        // PoB stores gem colour as its own escape code, not a letter. Reading
        // it as a letter silently yields None for every gem, which shows up
        // as socket chips vanishing from the UI rather than as an error.
        assert_eq!(SocketColor::parse("^xE05030"), Some(SocketColor::Red));
        assert_eq!(SocketColor::parse("^x70FF70"), Some(SocketColor::Green));
        assert_eq!(SocketColor::parse("^x7070FF"), Some(SocketColor::Blue));
        assert_eq!(SocketColor::parse("^xC8C8C8"), Some(SocketColor::White));
        assert_eq!(SocketColor::parse("^x123456"), None);
    }

    #[test]
    fn the_escape_codes_round_trip_to_the_same_rgb() {
        // The parsed colour must paint as the hex PoB gave us, otherwise the
        // chip would be a different red from the one PoB shows.
        for (code, expect) in [
            ("^xE05030", (0xE0, 0x50, 0x30)),
            ("^x70FF70", (0x70, 0xFF, 0x70)),
            ("^x7070FF", (0x70, 0x70, 0xFF)),
        ] {
            assert_eq!(SocketColor::parse(code).unwrap().rgb(), expect);
        }
    }

    #[test]
    fn socket_letters_carry_meaning_without_colour() {
        for (s, expect) in [
            ("R", SocketColor::Red),
            ("g", SocketColor::Green),
            ("B", SocketColor::Blue),
            ("W", SocketColor::White),
        ] {
            let c = SocketColor::parse(s).expect("parses");
            assert_eq!(c, expect);
            assert!(!c.letter().is_empty());
        }
        assert_eq!(SocketColor::parse("x"), None);
    }
}

/// The main-skill controls PoB draws under its socket-group dropdown, as
/// `RefreshSkillSelectControls` fills them. Lists are empty when PoB would
/// hide the control; indices are 1-based, as PoB's own are.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MainSkillControls {
    /// The active skills in the main group, when there is more than one.
    pub active_skills: Vec<String>,
    pub active_index: usize,
    /// Skill parts (e.g. "Idle" / "Channelling"), when there is more than one.
    pub parts: Vec<String>,
    pub part_index: usize,
    /// Stage count, when the skill or its selected part has stages.
    pub stages: Option<i64>,
    /// Active mine count, for mine skills.
    pub mines: Option<i64>,
    /// Minion choices, when the skill has minions. Values are what the
    /// setter takes back: a minion id, or an item-set id for spectre-like
    /// skills that use an item set.
    pub minions: Vec<String>,
    pub minion_values: Vec<String>,
    pub minion_index: usize,
    pub minion_by_item_set: bool,
    /// The minion's own skills, when a minion is selected.
    pub minion_skills: Vec<String>,
    pub minion_skill_index: usize,
}

/// One choice in PoB's node-power dropdown (`data.powerStatList`).
#[derive(Clone, Debug, PartialEq)]
pub struct PowerStat {
    /// PoB's stat key. Empty means the combined Offence/Defence measure.
    pub stat: String,
    pub label: String,
}

/// One row of PoB's Power Report, as `TreeTab:BuildPowerReportList` builds
/// it: already formatted with the stat's own precision and sign.
#[derive(Clone, Debug, PartialEq)]
pub struct PowerReportRow {
    pub id: u64,
    pub name: String,
    /// Normal, Notable, Keystone, Mastery, or the cluster notable's type.
    pub kind: String,
    /// Raw value, for sorting and colouring.
    pub power: f64,
    pub power_text: String,
    /// Per point along the path to the node; "--" for cluster notables.
    pub per_point_text: String,
    /// Points to reach it, or "Cluster".
    pub points_text: String,
    pub allocated: bool,
    /// Lower is better for this stat (PoB's `lowerIsBetter`), so a negative
    /// value is the good direction.
    pub lower_is_better: bool,
}
