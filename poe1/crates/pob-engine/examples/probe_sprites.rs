use pob_engine::Engine;
use std::path::PathBuf;

fn main() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding/src").canonicalize().unwrap();
    let engine = Engine::new(&src).expect("boot");
    engine.load_build_xml(include_str!("../tests/fixtures/winter-orb-85.xml")).expect("load");

    let report = engine.eval::<String>(r#"
        local out = {}
        local function p(...) out[#out+1] = table.concat({...}, " ") end
        local t = __mainObject__.main.modes["BUILD"].spec.tree

        local sheets = {}
        for kind, byZoom in pairs(t.skillSprites or {}) do
            local top = byZoom[#byZoom]
            if top then sheets[#sheets+1] = kind .. " -> " .. tostring(top.filename) end
        end
        table.sort(sheets)
        p("skillSprites (" .. #sheets .. "):")
        for i = 1, math.min(8, #sheets) do p("  " .. sheets[i]) end

        for _, name in ipairs({"PSSkillFrame", "NotableFrameUnallocated",
                               "PSGroupBackground3", "Background2", "ClassesOccultist"}) do
            local a = t.assets and t.assets[name]
            if a then
                local ak = {}
                for k, v in pairs(a) do ak[#ak+1] = k .. "=" .. tostring(v) end
                table.sort(ak)
                p("asset " .. name .. ": " .. table.concat(ak, ", "):sub(1, 200))
            else
                p("asset " .. name .. ": MISSING")
            end
        end

        for id, node in pairs(t.nodes or {}) do
            if node.type == "Notable" and node.sprites then
                local sk = {}
                for k in pairs(node.sprites) do sk[#sk+1] = tostring(k) end
                table.sort(sk)
                p("node sprite keys: " .. table.concat(sk, ", "))
                local s = node.sprites.notableActive or node.sprites.notableInactive
                if s then
                    local kk = {}
                    for k, v in pairs(s) do kk[#kk+1] = k .. "=" .. tostring(v) end
                    table.sort(kk)
                    p("  sprite fields: " .. table.concat(kk, ", "):sub(1, 300))
                end
                break
            end
        end
        return table.concat(out, "\n")
    "#).expect("probe");
    println!("{report}");
}
