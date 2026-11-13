//! Find Timeless Jewel, by driving PoB's own dialog.
//!
//! PoB's finder is 1,700 lines of `TreeTab:FindTimelessJewel`: the jewel
//! and conqueror tables, the seed loop over the lookup tables, the weighting
//! rules per jewel type, the trade-site query and the item text it adds to a
//! build. All of it lives inside one function as locals captured by the
//! popup's controls. Rather than port that and drift from it each league, we
//! open the popup headless, set its controls the way a user would, press
//! its buttons, and read what it wrote to `build.timelessData`. The results
//! are PoB's to the byte, and a league that changes the weighting changes
//! ours for free.

use crate::{Engine, Error};
use mlua::Table;

/// A timeless jewel kind, e.g. Lethal Pride. Ids are PoB's.
#[derive(Clone, Debug, PartialEq)]
pub struct JewelType {
    pub id: u8,
    pub label: String,
}

/// A conqueror (keystone variant) of the selected jewel type. Id 1 is "Any".
#[derive(Clone, Debug, PartialEq)]
pub struct Conqueror {
    pub id: u8,
    pub label: String,
}

/// A jewel socket the search can target. Id -1 searches every socket.
#[derive(Clone, Debug, PartialEq)]
pub struct TimelessSocket {
    pub id: i64,
    /// PoB's label: the nearest keystone and the node id, e.g.
    /// "Templar/Witch: 7960".
    pub label: String,
    pub keystone: String,
    /// The socket is allocated on the active tree. PoB marks these "#".
    pub allocated: bool,
}

/// A legion passive that can be searched for: a notable, a small passive's
/// stat, or the synthetic "Total Strength/Dexterity/Devotion".
#[derive(Clone, Debug, PartialEq)]
pub struct LegionNode {
    /// PoB's id, e.g. `karui_notable_add_strength`. What the search list
    /// stores.
    pub id: String,
    pub name: String,
    /// Stat lines, as PoB shows in the dropdown's tooltip.
    pub stats: Vec<String>,
    pub notable: bool,
    /// The hybrid total entry PoB puts at the top for Lethal Pride, Brutal
    /// Restraint and Militant Faith.
    pub total: bool,
    /// How many stats the node rolls. Two or more means the secondary weight
    /// applies to the second stat.
    pub stat_count: usize,
    /// What the primary and secondary weights weigh, for the sliders' help.
    pub primary_stat: String,
    pub secondary_stat: String,
}

/// One line of PoB's desired or fallback node list. Weights are kept as PoB
/// keeps them, text, because "required" is a valid value alongside numbers.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DesiredNode {
    pub id: String,
    pub weight1: String,
    pub weight2: String,
    pub minimum: String,
}

impl DesiredNode {
    /// Parse PoB's `id, w1, w2, min` line.
    pub fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .collect();
        let id = (*parts.first()?).to_owned();
        Some(Self {
            id,
            weight1: parts.get(1).map(|s| (*s).to_owned()).unwrap_or_default(),
            weight2: parts.get(2).map(|s| (*s).to_owned()).unwrap_or_default(),
            minimum: parts.get(3).map(|s| (*s).to_owned()).unwrap_or_default(),
        })
    }

    /// Back to PoB's line form.
    pub fn line(&self) -> String {
        format!(
            "{}, {}, {}, {}",
            self.id,
            if self.weight1.is_empty() { "0" } else { &self.weight1 },
            if self.weight2.is_empty() { "0" } else { &self.weight2 },
            if self.minimum.is_empty() { "0" } else { &self.minimum }
        )
    }
}

fn parse_list(text: &str) -> Vec<DesiredNode> {
    text.lines().filter_map(DesiredNode::parse).collect()
}

fn list_text(rows: &[DesiredNode]) -> String {
    rows.iter().map(DesiredNode::line).collect::<Vec<_>>().join("\n")
}

/// The search settings PoB saves in the build (`<TimelessData>`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TimelessState {
    pub jewel_type: u8,
    pub conqueror: u8,
    pub devotion1: u8,
    pub devotion2: u8,
    pub socket: i64,
    /// Only score nodes the build has allocated (plus those within
    /// `socket_distance` points of the tree).
    pub socket_filter: bool,
    pub socket_distance: u32,
    pub desired: Vec<DesiredNode>,
    pub fallback: Vec<DesiredNode>,
    /// Index into [`TimelessOptions::fallback_modes`].
    pub fallback_mode: u32,
    pub total_minimum_weight: Option<f64>,
}

/// Everything the dialog offers, for the current state's jewel type.
#[derive(Clone, Debug, Default)]
pub struct TimelessOptions {
    pub jewel_types: Vec<JewelType>,
    /// Conquerors of the selected jewel type.
    pub conquerors: Vec<Conqueror>,
    pub devotion_variants: Vec<Conqueror>,
    pub sockets: Vec<TimelessSocket>,
    /// Searchable nodes of the selected jewel type, in PoB's dropdown order:
    /// the total entry, notables, then small passives.
    pub nodes: Vec<LegionNode>,
    /// "Sort by Full DPS" and the rest of PoB's power stats.
    pub fallback_modes: Vec<String>,
    /// Allocated notables in the selected socket's radius that Militant Faith
    /// or Reclaimed Malevolence can be told not to replace. Empty unless the
    /// socket filter is on, as in PoB.
    pub protect_candidates: Vec<String>,
    /// Reclaimed Malevolence's "required ascendancy outcome" choices.
    pub abyss_ascendancies: Vec<LegionNode>,
}

/// How one desired node fared for a seed.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeMatch {
    pub name: String,
    pub weight: f64,
    /// Which passives in radius the seed turned into this.
    pub targets: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelessResult {
    pub seed: i64,
    pub total: f64,
    /// Set by a multi-socket search.
    pub socket_id: Option<i64>,
    pub socket_label: String,
    /// One per desired node, in the desired list's order.
    pub matches: Vec<NodeMatch>,
    /// Already added to the build from this result list.
    pub added: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TimelessSearch {
    /// Column headers: the desired nodes, in order.
    pub desired: Vec<String>,
    /// Best first, as PoB sorts them.
    pub results: Vec<TimelessResult>,
}

/// Opens the dialog headless around a chunk that receives `controls` and
/// `td` (the build's timelessData) and returns what the chunk returns. The
/// popup is closed whether or not the chunk fails.
const OPEN: &str = r#"
    local b = __mainObject__.main.modes["BUILD"]
    if not (b and b.spec and b.treeTab) then return nil end
    local td = b.timelessData
    local before = #__mainObject__.main.popups
    b.treeTab:FindTimelessJewel()
    local popup = __mainObject__.main.popups[1]
    local controls = popup.controls
    local ok, result = pcall(body, b, td, controls)
    while #__mainObject__.main.popups > before do
        __mainObject__.main:ClosePopup()
    end
    if not ok then error(result, 0) end
    return result
"#;

impl Engine {
    /// Run `body(b, td, controls)` inside PoB's opened dialog.
    fn with_timeless_popup<T: mlua::FromLuaMulti>(&self, body: &str) -> Result<T, Error> {
        let chunk = format!("{READ_RESULTS}\nlocal body = function(b, td, controls)\n{body}\nend\n{OPEN}");
        let out = self.lua.load(&chunk).eval::<T>()?;
        Self::check_prompt(&self.lua)?;
        Ok(out)
    }

    /// The saved search settings.
    pub fn timeless_state(&self) -> Result<TimelessState, Error> {
        let t: Table = self.chunk(
                r#"
                local b = __mainObject__.main.modes["BUILD"]
                local td = b and b.timelessData
                if not td then return {} end
                return {
                    jewelType = td.jewelType and td.jewelType.id or 1,
                    conqueror = td.conquerorType and td.conquerorType.id or 1,
                    devotion1 = td.devotionVariant1 or 1,
                    devotion2 = td.devotionVariant2 or 1,
                    socket = td.jewelSocket and td.jewelSocket.id or -1,
                    socketFilter = td.socketFilter and true or false,
                    socketDistance = td.socketFilterDistance or 0,
                    searchList = td.searchList or "",
                    fallbackList = td.searchListFallback or "",
                    fallbackMode = td.fallbackWeightMode and td.fallbackWeightMode.idx or 1,
                    totalMin = td.totalMinimumWeight,
                }
                "#,
            )?.call(())?;
        Ok(TimelessState {
            jewel_type: t.get::<Option<u8>>("jewelType")?.unwrap_or(1),
            conqueror: t.get::<Option<u8>>("conqueror")?.unwrap_or(1),
            devotion1: t.get::<Option<u8>>("devotion1")?.unwrap_or(1),
            devotion2: t.get::<Option<u8>>("devotion2")?.unwrap_or(1),
            socket: t.get::<Option<i64>>("socket")?.unwrap_or(-1),
            socket_filter: t.get::<Option<bool>>("socketFilter")?.unwrap_or(false),
            socket_distance: t.get::<Option<u32>>("socketDistance")?.unwrap_or(0),
            desired: parse_list(&t.get::<Option<String>>("searchList")?.unwrap_or_default()),
            fallback: parse_list(&t.get::<Option<String>>("fallbackList")?.unwrap_or_default()),
            fallback_mode: t.get::<Option<u32>>("fallbackMode")?.unwrap_or(1),
            total_minimum_weight: t.get::<Option<f64>>("totalMin")?,
        })
    }

    /// Store search settings on the build, where PoB saves them.
    ///
    /// Written as given. PoB's dropdown empties both node lists when the
    /// jewel type changes, because the ids are per type; that is the
    /// caller's rule to apply, since it is the caller that knows a change
    /// was the user's.
    pub fn set_timeless_state(&self, state: &TimelessState) -> Result<(), Error> {
        let f: mlua::Function = self.chunk(
                r#"
                local s = ...
                local b = __mainObject__.main.modes["BUILD"]
                local td = b and b.timelessData
                if not td then return end
                td.jewelType = { id = s.jewelType }
                td.conquerorType = { id = s.conqueror }
                td.devotionVariant1 = s.devotion1
                td.devotionVariant2 = s.devotion2
                td.jewelSocket = { id = s.socket }
                td.socketFilter = s.socketFilter
                td.socketFilterDistance = s.socketDistance
                td.searchList = s.searchList
                td.searchListFallback = s.fallbackList
                td.fallbackWeightMode = { idx = s.fallbackMode }
                td.totalMinimumWeight = s.totalMin
                b.modFlag = true
                "#,
            )?;
        let t = self.lua.create_table()?;
        t.set("jewelType", state.jewel_type)?;
        t.set("conqueror", state.conqueror)?;
        t.set("devotion1", state.devotion1)?;
        t.set("devotion2", state.devotion2)?;
        t.set("socket", state.socket)?;
        t.set("socketFilter", state.socket_filter)?;
        t.set("socketDistance", state.socket_distance)?;
        t.set("searchList", list_text(&state.desired))?;
        t.set("fallbackList", list_text(&state.fallback))?;
        t.set("fallbackMode", state.fallback_mode)?;
        t.set("totalMin", state.total_minimum_weight)?;
        f.call::<()>(t)?;
        Self::check_prompt(&self.lua)
    }

    /// What the dialog offers for the current settings.
    pub fn timeless_options(&self) -> Result<TimelessOptions, Error> {
        let t: Option<Table> = self.with_timeless_popup(
            r##"
            local legion = b.spec.tree.legion
            local function statInfo(id)
                for _, list in ipairs({ legion.nodes, legion.additions }) do
                    for _, n in ipairs(list) do
                        if n.id == id then
                            local count = td.jewelType.id >= 7 and #(n.sortedStats or {}) or #(n.sd or {})
                            local sd = n.sd or {}
                            if count > #sd then
                                return count, "Minimum value: " .. (sd[1] or ""), "Maximum value: " .. (sd[1] or "")
                            end
                            return count,
                                count == 1 and table.concat(sd, " + ") or sd[1] or "None",
                                sd[2] or "None"
                        end
                    end
                end
                return 0, "None", "None"
            end
            local function nodeList(list)
                local out = {}
                for _, m in ipairs(list) do
                    if m.id then
                        local count, s1, s2 = statInfo(m.id)
                        if m.totalMod then count = 1 end
                        out[#out + 1] = {
                            id = m.id,
                            -- PoB pads notable labels with spaces before the
                            -- stat text; the stats come separately here.
                            name = tostring(m.label):gsub("%s%s+.*$", ""),
                            stats = m.descriptions or {},
                            notable = m.label ~= m.id and tostring(m.label):find("%s%s+") ~= nil,
                            total = m.totalMod and true or false,
                            statCount = count,
                            stat1 = s1,
                            stat2 = s2,
                        }
                    end
                end
                return out
            end
            local out = {
                jewelTypes = {}, conquerors = {}, devotion = {}, sockets = {},
                nodes = nodeList(controls.nodeSelect.list),
                fallbackModes = {}, protect = {}, abyss = {},
            }
            for _, j in ipairs(controls.jewelSelect.list) do
                out.jewelTypes[#out.jewelTypes + 1] = { id = j.id, label = j.label }
            end
            for _, c in ipairs(controls.conquerorSelect.list) do
                out.conquerors[#out.conquerors + 1] = { id = c.id, label = c.label }
            end
            for _, d in ipairs(controls.devotionSelect1.list) do
                out.devotion[#out.devotion + 1] = { id = d.id, label = d.label }
            end
            for _, s in ipairs(controls.socketSelect.list) do
                local label = tostring(s.label)
                local allocated = label:sub(1, 2) == "# "
                out.sockets[#out.sockets + 1] = {
                    id = s.id, label = allocated and label:sub(3) or label,
                    keystone = s.keystone or "", allocated = allocated,
                }
            end
            for _, m in ipairs(controls.fallbackWeightsList.list) do
                out.fallbackModes[#out.fallbackModes + 1] = m.label
            end
            for _, p in ipairs(controls.protectAllocatedSelect.list or {}) do
                out.protect[#out.protect + 1] = p.label
            end
            for _, a in ipairs(controls.abyssAscendancySelect.list or {}) do
                if a.id then
                    out.abyss[#out.abyss + 1] = {
                        id = a.id, name = a.label, stats = a.descriptions or {},
                        notable = true, total = false, statCount = 1, stat1 = "", stat2 = "",
                    }
                end
            end
            return out
            "##,
        )?;
        let Some(t) = t else { return Ok(TimelessOptions::default()) };

        fn pairs(t: &Table, key: &str) -> Result<Vec<Conqueror>, Error> {
            let list: Table = t.get(key)?;
            let mut out = Vec::new();
            for row in list.sequence_values::<Table>() {
                let row = row?;
                out.push(Conqueror { id: row.get("id")?, label: row.get("label")? });
            }
            Ok(out)
        }
        fn nodes(t: &Table, key: &str) -> Result<Vec<LegionNode>, Error> {
            let list: Table = t.get(key)?;
            let mut out = Vec::new();
            for row in list.sequence_values::<Table>() {
                let row = row?;
                let stats: Table = row.get("stats")?;
                out.push(LegionNode {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    stats: stats.sequence_values::<String>().collect::<Result<_, _>>()?,
                    notable: row.get("notable")?,
                    total: row.get("total")?,
                    stat_count: row.get("statCount")?,
                    primary_stat: row.get("stat1")?,
                    secondary_stat: row.get("stat2")?,
                });
            }
            Ok(out)
        }

        let mut out = TimelessOptions {
            jewel_types: pairs(&t, "jewelTypes")?
                .into_iter()
                .map(|c| JewelType { id: c.id, label: c.label })
                .collect(),
            conquerors: pairs(&t, "conquerors")?,
            devotion_variants: pairs(&t, "devotion")?,
            nodes: nodes(&t, "nodes")?,
            abyss_ascendancies: nodes(&t, "abyss")?,
            ..Default::default()
        };
        let sockets: Table = t.get("sockets")?;
        for row in sockets.sequence_values::<Table>() {
            let row = row?;
            out.sockets.push(TimelessSocket {
                id: row.get("id")?,
                label: row.get("label")?,
                keystone: row.get("keystone")?,
                allocated: row.get("allocated")?,
            });
        }
        let modes: Table = t.get("fallbackModes")?;
        out.fallback_modes = modes.sequence_values::<String>().collect::<Result<_, _>>()?;
        let protect: Table = t.get("protect")?;
        out.protect_candidates = protect.sequence_values::<String>().collect::<Result<_, _>>()?;
        Ok(out)
    }

    /// Run the search with the saved settings. `protected` names notables
    /// (from [`TimelessOptions::protect_candidates`]) that a Militant Faith
    /// or Reclaimed Malevolence seed must not replace.
    pub fn timeless_search(&self, protected: &[String]) -> Result<TimelessSearch, Error> {
        let protect_table = self.lua.create_table()?;
        for (i, name) in protected.iter().enumerate() {
            protect_table.set(i + 1, name.as_str())?;
        }
        self.lua.globals().set("__pobios_protect", protect_table)?;
        let t: Option<Table> = self.with_timeless_popup(
            r#"
            for _, name in ipairs(__pobios_protect or {}) do
                for i, p in ipairs(controls.protectAllocatedSelect.list or {}) do
                    if p.label == name then
                        controls.protectAllocatedSelect.selIndex = i
                        controls.protectAllocatedButtonAdd.onClick()
                        break
                    end
                end
            end
            controls.searchButton.onClick()
            return readResults(td)
            "#,
        )?;
        self.lua.globals().set("__pobios_protect", mlua::Nil)?;
        Self::parse_results(t)
    }

    /// The last search's results, re-read without searching again. What
    /// changes between reads is the `added` flag.
    pub fn timeless_results(&self) -> Result<TimelessSearch, Error> {
        let t: Option<Table> = self.with_timeless_popup("return readResults(td)")?;
        Self::parse_results(t)
    }

    fn parse_results(t: Option<Table>) -> Result<TimelessSearch, Error> {
        let Some(t) = t else { return Ok(TimelessSearch::default()) };

        let desired: Table = t.get("desired")?;
        let desired: Vec<String> = desired.sequence_values::<String>().collect::<Result<_, _>>()?;
        let mut out = TimelessSearch { desired: desired.clone(), results: Vec::new() };
        let results: Table = t.get("results")?;
        for row in results.sequence_values::<Table>() {
            let row = row?;
            let matches: Table = row.get("matches")?;
            let mut ms = Vec::new();
            for (i, m) in matches.sequence_values::<Table>().enumerate() {
                let m = m?;
                let targets: Table = m.get("targets")?;
                ms.push(NodeMatch {
                    name: desired.get(i).cloned().unwrap_or_default(),
                    weight: m.get("weight")?,
                    targets: targets.sequence_values::<String>().collect::<Result<_, _>>()?,
                });
            }
            let socket_label: String = row.get("socketLabel")?;
            out.results.push(TimelessResult {
                seed: row.get("seed")?,
                total: row.get("total")?,
                socket_id: row.get::<Option<i64>>("socketId")?,
                socket_label: socket_label.trim_start_matches("# ").to_owned(),
                matches: ms,
                added: row.get("added")?,
            });
        }
        Ok(out)
    }
}

/// Lua: the last search's results as plain tables, in desired-node order.
/// Defined once alongside the popup opener so both search and re-read use it.
const READ_RESULTS: &str = r#"
    local function readResults(td)
            local out = { desired = {}, results = {} }
            local shared = td.sharedResults or {}
            -- PoB heads each column with the stat text; a column here is
            -- 84pt wide, so use the legion node's name and keep the stat
            -- text for Devotion and the totals, which have no node.
            local names = {}
            local legion = __mainObject__.main.modes["BUILD"].spec.tree.legion or {}
            for _, n in ipairs(legion.nodes or {}) do names[n.id] = n.dn end
            for _, n in ipairs(legion.additions or {}) do names[n.id] = names[n.id] or n.dn end
            local order = {}
            for legionId, d in pairs(shared.desiredNodes or {}) do
                order[#order + 1] = { id = legionId, idx = d.desiredIdx, name = names[legionId] or d.displayName }
            end
            table.sort(order, function(a, c) return a.idx < c.idx end)
            for _, d in ipairs(order) do out.desired[#out.desired + 1] = d.name end
            for _, r in ipairs(td.searchResults or {}) do
                local label = tostring(r.label)
                local added = label:sub(1, 8) == "^xB2B2B2"
                if added then label = label:sub(9) end
                -- The per-node weights live only in the label PoB formats:
                -- after "<seed>:" come the values in desired order, "0"
                -- for a miss and "..." past the seventh.
                local values = {}
                local seedTag = tostring(r.seed) .. ":"
                local seen = false
                for token in label:gmatch("%S+") do
                    if seen then values[#values + 1] = token
                    elseif token == seedTag then seen = true end
                end
                local matches = {}
                for i, d in ipairs(order) do
                    local m = r[d.id]
                    matches[i] = {
                        weight = tonumber(values[i]) or 0,
                        targets = m and m.targetNodeNames or {},
                    }
                end
                out.results[#out.results + 1] = {
                    seed = r.seed, total = r.total,
                    socketId = r.socketId, socketLabel = r.socketLabel or "",
                    matches = matches, added = added,
                }
            end
            return out
    end
"#;

impl Engine {
    /// PoB's "Generate": weight every searchable node by what it does for
    /// the chosen stat, and store the list as the fallback nodes.
    ///
    /// Runs a calculation per node, so it takes a moment.
    pub fn timeless_generate_fallback(&self) -> Result<Vec<DesiredNode>, Error> {
        let text: Option<String> = self.with_timeless_popup(
            r#"
            controls.fallbackWeightsButton.onClick()
            return td.searchListFallback or ""
            "#,
        )?;
        Ok(parse_list(&text.unwrap_or_default()))
    }

    /// The trade-site URL PoB would open for the results from `start`
    /// (0-based) on, exactly as its "Open Trade URL" button counts them:
    /// `scope` 0 is that one seed (PoB's shift-click range of one row), 1
    /// the ten or so from it, 2 up to 180.
    ///
    /// `league` is typed by the user; PoB fetches the list from the trade
    /// site and this app has no network. `realm` indexes PC, Sony, Xbox;
    /// `buyout` PoB's five listing filters, instant buyout first.
    pub fn timeless_trade_url(
        &self,
        start: usize,
        scope: u8,
        league: &str,
        realm: u8,
        buyout: u8,
    ) -> Result<Option<String>, Error> {
        let args = self.lua.create_table()?;
        args.set("start", start + 1)?;
        args.set("many", scope >= 2)?;
        args.set("single", scope == 0)?;
        // PoB pastes the league into the URL path raw; a space in a league
        // name ("Hardcore Settlers") makes a URL iOS refuses to open.
        let league: String = league
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect();
        args.set("league", league)?;
        args.set("realm", realm + 1)?;
        args.set("buyout", buyout + 1)?;
        self.lua.globals().set("__pobios_trade", args)?;
        let url: Option<String> = self.with_timeless_popup(
            r#"
            local a = __pobios_trade
            if not td.searchResults or #td.searchResults == 0 then return nil end
            if a.start > #td.searchResults then return nil end
            local captured
            local realOpen, realCopy = OpenURL, Copy
            OpenURL = function(url) captured = url end
            Copy = function() end
            local ok, err = pcall(function()
                controls.realmSelection:SetSel(a.realm)
                controls.searchTradeLeagueSelect:SetList({ a.league })
                controls.searchTradeLeagueSelect:SetSel(1)
                controls.tradeTypeSelection:SetSel(a.buyout)
                controls.searchMore.state = a.many
                controls.searchResults.selIndex = a.start
                -- A highlighted row bounds the range, as shift-click does.
                controls.searchResults.highlightIndex = a.single and a.start or nil
                controls.searchTradeButton.lastSearch = nil
                controls.searchTradeButton.onClick()
            end)
            OpenURL, Copy = realOpen, realCopy
            if not ok then error(err, 0) end
            return captured
            "#,
        )?;
        self.lua.globals().set("__pobios_trade", mlua::Nil)?;
        Ok(url)
    }

    /// Add the jewel a result describes to the build's items, as PoB's
    /// double-click does, and socket it where the search looked. `index` is
    /// 0-based into the last search's results.
    ///
    /// PoB stops at the item list and leaves the socketing to you; with a
    /// list of identically named jewels that is a guessing game on a
    /// tablet, and the search already knows the socket. Returns the socket's
    /// label, empty when the search spanned every socket and nothing was
    /// socketed, or `None` when nothing was added.
    pub fn timeless_add_to_build(&self, index: usize) -> Result<Option<String>, Error> {
        self.lua.globals().set("__pobios_add", index + 1)?;
        let out: Option<String> = self.with_timeless_popup(
            r#"
            local i = __pobios_add
            local data = td.searchResults and td.searchResults[i]
            if not data then return nil end
            local tab = b.itemsTab
            local before = {}
            for id in pairs(tab.items) do before[id] = true end
            controls.searchResults:OnSelClick(i, data, true)
            local added
            for id in pairs(tab.items) do
                if not before[id] and (not added or id > added) then added = id end
            end
            if not added then return nil end
            local socketId = data.socketId or (td.jewelSocket and td.jewelSocket.id)
            local slot = socketId and socketId > 0 and tab.slots["Jewel " .. socketId]
            local label = ""
            if slot then
                slot:SetSelItemId(added)
                tab:PopulateSlots()
                tab:AddUndoState()
                label = tostring(slot.label or slot.slotName or ("Jewel " .. socketId))
            end
            b.buildFlag = true
            runCallback("OnFrame")
            return label
            "#,
        )?;
        self.lua.globals().set("__pobios_add", mlua::Nil)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_lines_round_trip_through_pobs_form() {
        let n = DesiredNode::parse("karui_notable_add_strength, 1, 0.5, required").unwrap();
        assert_eq!(n.id, "karui_notable_add_strength");
        assert_eq!(n.weight1, "1");
        assert_eq!(n.weight2, "0.5");
        assert_eq!(n.minimum, "required");
        assert_eq!(n.line(), "karui_notable_add_strength, 1, 0.5, required");

        // A bare id gets zero weights rather than a malformed line.
        let bare = DesiredNode::parse("vaal_x").unwrap();
        assert_eq!(bare.line(), "vaal_x, 0, 0, 0");
        assert!(DesiredNode::parse("   ").is_none());
    }
}
