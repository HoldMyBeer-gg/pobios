//! Import a character from a pathofexile.com account, through PoB's own
//! ImportTab.
//!
//! The network is not here. The app signs in (OAuth, PKCE, PoB's public
//! `pob` client) and fetches `/character/{name}` from the API itself; what
//! arrives is handed to PoB's `ImportPassiveTreeAndJewels` and
//! `ImportItemsAndSkills` unchanged, so what a character turns into is
//! PoB's reading of it, cluster jewels, tattoos, pantheon and all.

use crate::{Engine, Error};

impl Engine {
    /// Replace the tree with the character's, as PoB's "Passive Tree and
    /// Jewels" button does. `json` is the API's `/character/{name}`
    /// response, with or without its `character` wrapper. `delete_jewels`
    /// is PoB's "Delete jewels" checkbox.
    pub fn import_character_tree(&self, json: &str, delete_jewels: bool) -> Result<(), Error> {
        self.import_character(json, "tree", &[delete_jewels])
    }

    /// Import the character's items and skills, as PoB's "Items and Skills"
    /// button does, with its three checkboxes.
    pub fn import_character_items(
        &self,
        json: &str,
        clear_items: bool,
        clear_skills: bool,
        ignore_weapon_swap: bool,
    ) -> Result<(), Error> {
        self.import_character(json, "items", &[clear_items, clear_skills, ignore_weapon_swap])
    }

    fn import_character(&self, json: &str, what: &str, flags: &[bool]) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local json, what, a, b, c = ...
                local dkjson = require("dkjson")
                local data = dkjson.decode(json)
                local char = data and (data.character or data)
                if type(char) ~= "table" or type(char.passives) ~= "table" then
                    error("That is not a character from the Path of Exile API.", 0)
                end
                -- PoB's importers delete jewels, reset the tree and wipe
                -- gear before they read these, so a character missing them
                -- would leave the build gutted. Refuse up front.
                if type(char.class) ~= "string" or type(char.league) ~= "string"
                    or type(char.passives.hashes) ~= "table" then
                    error("That character is missing its class, league or passives.", 0)
                end
                if what ~= "tree" and type(char.equipment) ~= "table" then
                    error("That character has no equipment list.", 0)
                end
                local build = __mainObject__.main.modes["BUILD"]
                local tab = build and build.importTab
                if not tab then error("No build loaded.", 0) end
                if what == "tree" then
                    tab:ImportPassiveTreeAndJewels(char, a)
                else
                    tab:ImportItemsAndSkills(char, a, b, c)
                end
                build.buildFlag = true
                runCallback("OnFrame")
                "#,
            )?;
        let flag = |i: usize| flags.get(i).copied().unwrap_or(false);
        f.call::<()>((json, what, flag(0), flag(1), flag(2)))?;
        Self::check_prompt(&self.lua)
    }
}
