//! Equipped items and their modifier lines.

/// Item rarity, which drives the name colour PoB uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Rarity {
    #[default]
    Normal,
    Magic,
    Rare,
    Unique,
    Relic,
}

impl Rarity {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "MAGIC" => Self::Magic,
            "RARE" => Self::Rare,
            "UNIQUE" => Self::Unique,
            "RELIC" => Self::Relic,
            _ => Self::Normal,
        }
    }

    /// PoB's own rarity colours from `Data/Global.lua` colorCodes.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Normal => (0xC8, 0xC8, 0xC8),
            Self::Magic => (0x88, 0x88, 0xFF),
            Self::Rare => (0xFF, 0xFF, 0x77),
            Self::Unique => (0xAF, 0x60, 0x25),
            Self::Relic => (0x60, 0xC0, 0x60),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Magic => "Magic",
            Self::Rare => "Rare",
            Self::Unique => "Unique",
            Self::Relic => "Relic",
        }
    }
}

/// Where a modifier line came from. The item editor groups lines by this, and
/// PoB stores each kind in its own table rather than tagging one list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModKind {
    Implicit,
    Explicit,
    Crafted,
    Enchant,
    Scourge,
    Crucible,
}

impl ModKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Implicit => "implicit",
            Self::Explicit => "explicit",
            Self::Crafted => "crafted",
            Self::Enchant => "enchant",
            Self::Scourge => "scourge",
            Self::Crucible => "crucible",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModLine {
    pub text: String,
    pub kind: ModKind,
    /// PoB parsed this line but could not interpret all of it, so it does not
    /// fully count towards the build. PoB paints these red and appends
    /// "(Not supported in PoB yet)" — worth reproducing, because a mod that
    /// silently does nothing is how someone trusts a number that is wrong.
    pub unsupported: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Item {
    pub name: String,
    pub base_name: String,
    pub rarity: Rarity,
    pub item_level: i64,
    pub quality: i64,
    pub mods: Vec<ModLine>,
    /// Level requirement, when the item has one.
    pub req_level: Option<i64>,
    /// Attribute requirements, as PoB stores them.
    pub req_str: Option<i64>,
    pub req_dex: Option<i64>,
    pub req_int: Option<i64>,
    /// The italic line at the foot of a unique. Flavour only, no mechanics,
    /// but its absence is one of the things that makes an item look wrong.
    pub flavour: Option<String>,
    /// The headline numbers PoB's tooltip shows: DPS, crit and speed for a
    /// weapon; armour, evasion, energy shield, ward, block for armour.
    /// `(label, value)`, formatted.
    pub stats: Vec<(String, String)>,
}

impl Item {
    /// Mod lines of one kind, for the grouped editor view.
    pub fn mods_of(&self, kind: ModKind) -> impl Iterator<Item = &ModLine> {
        self.mods.iter().filter(move |m| m.kind == kind)
    }

    /// A one-line summary for the item list, as PoB shows under each name.
    pub fn summary(&self, max_mods: usize) -> String {
        let parts: Vec<&str> = self
            .mods
            .iter()
            .filter(|m| m.kind == ModKind::Explicit || m.kind == ModKind::Implicit)
            .take(max_mods)
            .map(|m| m.text.as_str())
            .collect();
        parts.join(" · ")
    }
}

/// An equipment slot and what occupies it.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub name: String,
    /// What to call it on screen. For a tree socket ("Jewel 61419") this is
    /// the socket node's name, "Jewel Socket" or "Large Jewel Socket".
    pub label: String,
    pub item: Option<Item>,
    /// Whether a flask is active. `None` for slots that are not flasks —
    /// PoB puts the checkbox only on those.
    pub active: Option<bool>,
}

/// One entry in the build's item list, as the slot dropdowns and the item
/// list panel show it.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemEntry {
    /// PoB's item id. This is what identifies an item to the engine; names
    /// are not unique, and a build routinely holds several "New Item".
    pub id: i64,
    pub name: String,
    pub rarity: Rarity,
    /// Which slots this item is currently equipped in, by PoB's slot names.
    /// Empty means unused, which is what Del Unused acts on.
    pub equipped_in: Vec<String>,
}

/// Which kind of set a call refers to.
///
/// PoB scopes a build's data into named sets — one set of items, one of
/// skills, one of config — and a *loadout* is those sets sharing a title.
/// There is no loadout record: the name is the binding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetKind {
    Item,
    Skill,
    Config,
}

impl SetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Item => "Item set",
            Self::Skill => "Skill set",
            Self::Config => "Config set",
        }
    }
}

/// One named set.
#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    /// PoB's own id, which is what identifies a set — titles are not unique
    /// and a build can hold several called "Default".
    pub id: i64,
    pub title: String,
}

/// The sets of one kind, and which is live.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetList {
    pub sets: Vec<Set>,
    /// Id of the active set, or 0 when there is none.
    pub active: i64,
}

impl SetList {
    pub fn active_title(&self) -> Option<&str> {
        self.sets
            .iter()
            .find(|s| s.id == self.active)
            .map(|s| s.title.as_str())
    }
}

/// What equipping an item would do, for the hover tooltip.
///
/// PoB's item tooltip has two halves: the item as it looks in game, and this
/// — every stat that would change, against what is worn now. It is the half
/// that makes a database of gear you do not own worth browsing.
#[derive(Clone, Debug, Default)]
pub struct ItemPreview {
    /// The slot this was computed against, by PoB's slot name.
    pub slot: String,
    /// What is worn there now, if anything. PoB names it in the header:
    /// "(replacing New Item, Jade Amulet)".
    pub replacing: Option<String>,
    /// True when the hovered item is the one already equipped, in which case
    /// the numbers are for *removing* it and the wording flips entirely.
    pub is_equipped: bool,
    pub deltas: Vec<crate::tree::StatDelta>,
}

/// PoB's canonical equipment slots, in its own order
/// (`baseSlots` in `Classes/ItemsTab.lua`).
///
/// PoB's live `slots` table also holds every abyssal and jewel socket — 131
/// entries for this fixture — so filtering to these is what separates gear
/// from sockets.
pub const EQUIPMENT_SLOTS: &[&str] = &[
    "Weapon 1",
    "Weapon 2",
    "Helmet",
    "Body Armour",
    "Gloves",
    "Boots",
    "Amulet",
    "Ring 1",
    "Ring 2",
    "Ring 3",
    "Belt",
    "Graft 1",
    "Graft 2",
    "Flask 1",
    "Flask 2",
    "Flask 3",
    "Flask 4",
    "Flask 5",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn item_with(mods: Vec<(ModKind, &str)>) -> Item {
        // `..Default::default()` so a new field on Item does not break every
        // test that builds one — the tooltip work added four at once.
        Item {
            name: "Test".into(),
            base_name: "Base".into(),
            rarity: Rarity::Rare,
            item_level: 80,
            quality: 20,
            mods: mods
                .into_iter()
                .map(|(kind, text)| ModLine {
                    text: text.to_owned(),
                    kind,
                    unsupported: false,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn rarity_parses_pobs_own_spelling() {
        assert_eq!(Rarity::parse("UNIQUE"), Rarity::Unique);
        assert_eq!(Rarity::parse("rare"), Rarity::Rare);
        // An unknown rarity falls back to Normal rather than panicking, since
        // upstream may add one before we do.
        assert_eq!(Rarity::parse("MYTHIC"), Rarity::Normal);
    }

    #[test]
    fn mods_are_grouped_by_kind_for_the_editor() {
        let item = item_with(vec![
            (ModKind::Implicit, "+10 to Strength"),
            (ModKind::Explicit, "+137 to maximum Life"),
            (ModKind::Crafted, "+23 to Dexterity"),
        ]);
        assert_eq!(item.mods_of(ModKind::Explicit).count(), 1);
        assert_eq!(item.mods_of(ModKind::Crafted).count(), 1);
        assert_eq!(item.mods_of(ModKind::Enchant).count(), 0);
    }

    #[test]
    fn the_list_summary_shows_only_the_meaningful_mods() {
        // Crafted and enchant lines are excluded from the one-line summary:
        // the list is for recognising an item at a glance, and its identity
        // comes from its implicit and explicit rolls.
        let item = item_with(vec![
            (ModKind::Explicit, "+137 to maximum Life"),
            (ModKind::Crafted, "+23 to Dexterity"),
            (ModKind::Explicit, "56% increased Armour"),
        ]);
        assert_eq!(item.summary(4), "+137 to maximum Life · 56% increased Armour");
    }

    #[test]
    fn the_summary_is_capped_so_one_item_cannot_flood_the_list() {
        let item = item_with(vec![
            (ModKind::Explicit, "one"),
            (ModKind::Explicit, "two"),
            (ModKind::Explicit, "three"),
        ]);
        assert_eq!(item.summary(2), "one · two");
    }

    #[test]
    fn equipment_slots_exclude_sockets() {
        // The point of this list: PoB's live slot table mixes gear with every
        // abyssal socket, and only these are equipment.
        assert!(EQUIPMENT_SLOTS.contains(&"Body Armour"));
        assert!(!EQUIPMENT_SLOTS.iter().any(|s| s.contains("Abyssal")));
    }
}

/// One entry of PoB's item database (`main.uniqueDB` / `main.rareDB`).
#[derive(Clone, Debug, PartialEq)]
pub struct DbItem {
    pub name: String,
    pub base_name: String,
    /// PoB's item type, e.g. "Body Armour", "Wand".
    pub item_type: String,
    pub rarity: Rarity,
    pub league: Option<String>,
    pub req_level: Option<i64>,
}

/// A craftable base: PoB's `data.itemBaseLists[type]` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct CraftBase {
    pub item_type: String,
    pub name: String,
}
