//! Render the real tree canvas to a PNG, off-screen.
//!
//! The canvas is run through egui exactly as the app runs it, the output is
//! tessellated, and the triangles are rasterised here in software with the
//! textures egui was handed. What comes out is what the drawing code drew,
//! at any zoom, with no simulator or device in the loop, so a change to the
//! tree's look can be inspected before anyone is told it works.
//!
//! Images land in `poe2/decoded-icons/` (ignored by git).

use egui::epaint::{ImageData, Primitive};
use egui::{Color32, TextureId};
use pob_core::tree_art::TreeArt;
use pob_core::tree_view::{TreeCanvas, View};
use pob_engine::Engine;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const W: usize = 1600;
const H: usize = 1200;
/// Pixels to a point, as on the iPad: the canvas picks art by pixels.
const SCALE: f32 = 2.0;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Texture {
    size: [usize; 2],
    pixels: Vec<Color32>,
}

impl Texture {
    /// Bilinear sample at a normalised coordinate, clamped at the edges.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (self.size[0] as f32, self.size[1] as f32);
        let x = (u * w - 0.5).clamp(0.0, w - 1.0);
        let y = (v * h - 0.5).clamp(0.0, h - 1.0);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.size[0] - 1), (y0 + 1).min(self.size[1] - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize| self.pixels[y * self.size[0] + x].to_array();
        let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        let mut out = [0.0; 4];
        for i in 0..4 {
            let top = a[i] as f32 * (1.0 - fx) + b[i] as f32 * fx;
            let bottom = c[i] as f32 * (1.0 - fx) + d[i] as f32 * fx;
            out[i] = top * (1.0 - fy) + bottom * fy;
        }
        out
    }
}

fn render(view: &mut View, tree: &pob_engine::tree::Tree, src: &std::path::Path, name: &str) {
    // A texture handle belongs to the context that made it, so the art cache
    // is rebuilt for each context rather than carried between shots.
    let mut art = TreeArt::new(src);
    let art = &mut art;
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(W as f32, H as f32) / SCALE);
    let mut input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
    input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(SCALE);

    let mut textures: HashMap<TextureId, Texture> = HashMap::new();
    let mut last = None;
    // Frames until the art stops arriving: icons load on the first, the
    // large backdrops one a frame after that. Then one more to paint.
    let mut loaded = usize::MAX;
    for _ in 0..200 {
        let settled = art.loaded_count() == loaded;
        loaded = art.loaded_count();
        let matches = HashSet::new();
        let power = HashMap::new();
        let mut out = ctx.run_ui(input.clone(), |ui| {
            TreeCanvas {
                tree,
                view,
                art,
                matches: &matches,
                searching: false,
                power: &power,
                tooltip_scrollable: false,
                pinned: None,
            }
            .show(ui);
        });
        let deltas = out
            .textures_delta
            .set
            .iter()
            .flat_map(|(id, list)| list.iter().map(move |delta| (id, delta)));
        for (id, delta) in deltas {
            let ImageData::Color(image) = &delta.image;
            match delta.pos {
                None => {
                    textures.insert(*id, Texture { size: image.size, pixels: image.pixels.clone() });
                }
                Some([px, py]) => {
                    if let Some(t) = textures.get_mut(id) {
                        for y in 0..image.size[1] {
                            for x in 0..image.size[0] {
                                t.pixels[(py + y) * t.size[0] + px + x] =
                                    image.pixels[y * image.size[0] + x];
                            }
                        }
                    }
                }
            }
        }
        // Applied above; egui asserts a delta is not dropped unhandled.
        out.textures_delta.clear();
        last = Some(out);
        if settled {
            break;
        }
    }
    println!("{name}: {loaded} textures");
    let out = last.unwrap();
    let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);

    // Premultiplied RGBA, blended in gamma space as egui's own backends do.
    let mut frame = vec![[0.0f32; 4]; W * H];
    for clipped in primitives {
        let Primitive::Mesh(mesh) = clipped.primitive else { continue };
        let Some(texture) = textures.get(&mesh.texture_id) else { continue };
        let clip = clipped.clip_rect * SCALE;
        for tri in mesh.indices.chunks_exact(3) {
            let v: Vec<_> = tri.iter().map(|&i| mesh.vertices[i as usize]).collect();
            let at = |p: egui::Pos2| (p.to_vec2() * SCALE).to_pos2();
            let (a, b, c) = (at(v[0].pos), at(v[1].pos), at(v[2].pos));
            let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if area.abs() < 1e-6 {
                continue;
            }
            let min_x = a.x.min(b.x).min(c.x).max(clip.min.x).max(0.0).floor() as usize;
            let max_x = a.x.max(b.x).max(c.x).min(clip.max.x).min(W as f32).ceil() as usize;
            let min_y = a.y.min(b.y).min(c.y).max(clip.min.y).max(0.0).floor() as usize;
            let max_y = a.y.max(b.y).max(c.y).min(clip.max.y).min(H as f32).ceil() as usize;
            for y in min_y..max_y.min(H) {
                for x in min_x..max_x.min(W) {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
                    let w1 = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let u = v[0].uv.x * w0 + v[1].uv.x * w1 + v[2].uv.x * w2;
                    let t = v[0].uv.y * w0 + v[1].uv.y * w1 + v[2].uv.y * w2;
                    let texel = texture.sample(u, t);
                    let mut src = [0.0; 4];
                    for i in 0..4 {
                        let tint = v[0].color.to_array()[i] as f32 * w0
                            + v[1].color.to_array()[i] as f32 * w1
                            + v[2].color.to_array()[i] as f32 * w2;
                        src[i] = texel[i] * tint / 255.0;
                    }
                    let dst = &mut frame[y * W + x];
                    let keep = 1.0 - src[3] / 255.0;
                    for i in 0..4 {
                        dst[i] = src[i] + dst[i] * keep;
                    }
                }
            }
        }
    }

    let bytes: Vec<u8> = frame
        .iter()
        .flat_map(|p| [p[0] as u8, p[1] as u8, p[2] as u8, 255])
        .collect();
    let dir = root().join("decoded-icons");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("snapshot-{name}.png"));
    image::save_buffer(&path, &bytes, W as u32, H as u32, image::ExtendedColorType::Rgba8)
        .unwrap();
    println!("wrote {}", path.display());
}

#[test]
#[serial_test::serial]
fn snapshot_the_tree() {
    let src = root().join("PathOfBuilding-PoE2/src").canonicalize().expect("PoE2 checkout");
    let xml = std::fs::read_to_string(root().join("ios/PoB/Resources/sample-build.xml")).unwrap();
    let engine = Engine::new(&src).expect("boot");
    engine.load_build_xml(&xml).expect("load");
    let tree = engine.tree().expect("tree");

    // Centre on an allocated notable so the shot has taken and untaken nodes,
    // rings of every size, and the orange path between them.
    let focus = tree
        .nodes
        .iter()
        .find(|n| {
            n.allocated
                && n.ascendancy.is_none()
                && n.kind == pob_engine::tree::NodeKind::Notable
        })
        .expect("an allocated notable");
    println!("focus: {:?} at ({}, {})", focus.name, focus.x, focus.y);

    // The build's own ascendancy plate, out at the rim.
    let plate = tree
        .backdrops
        .iter()
        .find(|b| !b.dimmed && (b.x, b.y) != (0.0, 0.0))
        .expect("the build's ascendancy plate");

    // A filled socket, and the sockets the Voices jewel grants.
    let at = |id: u64| tree.node(id).map(|n| (n.x, n.y)).expect("node");

    for (name, center, zoom) in [
        ("mid", (focus.x, focus.y), 0.35),
        ("close", (focus.x, focus.y), 1.0),
        ("centre", (0.0, 0.0), 0.25),
        ("plate", (plate.x, plate.y), 0.3),
        ("whole", tree.center(), 0.035),
        ("socket", at(32763), 1.0),
        ("sinister", at(26178), 1.0),
        ("keystone", at(28492), 1.0),
        ("ascendancy", (plate.x, plate.y), 0.45),
    ] {
        let mut view = View { center, zoom };
        render(&mut view, &tree, &src, name);
    }
}

/// Swap the text of item `id` for `text`, leaving its place in the build.
fn with_item(xml: &str, id: u32, text: &str) -> String {
    let open = format!("<Item id=\"{id}\">");
    let start = xml.find(&open).expect("item") + open.len();
    let end = start + xml[start..].find("</Item>").expect("item end");
    format!("{}\n{}\n\t\t{}", &xml[..start], text.trim(), &xml[end..])
}

/// The sample build's jewels have no radius, so three are swapped for ones
/// that do: a timeless jewel, a plain radius, and a ring.
#[test]
#[serial_test::serial]
fn snapshot_the_rings() {
    let src = root().join("PathOfBuilding-PoE2/src").canonicalize().expect("PoE2 checkout");
    let xml = std::fs::read_to_string(root().join("ios/PoB/Resources/sample-build.xml")).unwrap();
    let xml = with_item(
        &xml,
        11,
        "Rarity: UNIQUE\nHeroic Tragedy\nTimeless Jewel\nLimited to: 1 Historic\nRadius: Very Large\n\
         Implicits: 0\nRemembrancing 1000 songworthy deeds by the line of Vorana\n\
         Passives in radius are Conquered by the Kalguur\nHistoric",
    );
    let xml = with_item(
        &xml,
        13,
        "Rarity: RARE\nViper Sliver\nTime-Lost Ruby\nRadius: Medium\nImplicits: 0\n\
         Notable Passive Skills in Radius also grant 10% increased Fire Damage",
    );
    let xml = with_item(
        &xml,
        14,
        "Rarity: UNIQUE\nControlled Metamorphosis\nDiamond\nLimited to: 1\nRadius: Variable\n\
         Implicits: 0\nOnly affects Passives in Medium Ring\n\
         Passives in Radius can be Allocated without being connected to your tree\n\
         -10% to all Elemental Resistances",
    );
    let engine = Engine::new(&src).expect("boot");
    engine.load_build_xml(&xml).expect("load");
    let tree = engine.tree().expect("tree");
    let at = |id: u64| tree.node(id).map(|n| (n.x, n.y)).expect("node");

    for (name, id) in [("ring-timeless", 32763), ("ring-plain", 54127), ("ring-annulus", 26196)] {
        let mut view = View { center: at(id), zoom: 0.28 };
        render(&mut view, &tree, &src, name);
    }
}
