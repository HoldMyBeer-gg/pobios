//! Composite a region of the PoE2 tree straight to a PNG, so the icon + frame
//! pipeline can be *seen*, not just counted. No egui, no GPU: decode each
//! node's frame and icon through the atlas and alpha-blend them onto a canvas
//! at their tree positions.

use image::{imageops, RgbaImage};
use pob_engine::Engine;
use std::path::PathBuf;

fn src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .unwrap()
}

fn sprite(atlas: &mut tree_assets::Atlas, key: &str) -> Option<RgbaImage> {
    let img = atlas.icon_rgba(key)?;
    RgbaImage::from_raw(img.width, img.height, img.pixels)
}

/// Alpha-blend a straight segment onto the canvas (a simple DDA line).
fn line(canvas: &mut RgbaImage, a: (i64, i64), b: (i64, i64), col: [u8; 3]) {
    let (w, h) = (canvas.width() as i64, canvas.height() as i64);
    let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    let steps = dx.max(dy).max(1);
    for i in 0..=steps {
        let x = a.0 + (b.0 - a.0) * i / steps;
        let y = a.1 + (b.1 - a.1) * i / steps;
        if x >= 0 && x < w && y >= 0 && y < h {
            let p = canvas.get_pixel_mut(x as u32, y as u32);
            // Blend at ~65% so lines read without overpowering the icons.
            for c in 0..3 {
                p.0[c] = ((p.0[c] as u16 * 35 + col[c] as u16 * 65) / 100) as u8;
            }
        }
    }
}

#[test]
#[serial_test::serial]
fn render_tree_region_to_png() {
    let engine = Engine::new(&src()).unwrap();
    let tree = engine.tree().unwrap();
    let dir = src().join("TreeData").join(&tree.version);
    let tree_lua = std::fs::read_to_string(dir.join("tree.lua")).unwrap();
    let mut atlas = tree_assets::Atlas::new(&dir, &tree_lua);

    // Centre on the mean node position and take a window tight enough that
    // node icons are legible.
    let (mut sx, mut sy) = (0f64, 0f64);
    for n in &tree.nodes {
        sx += n.x as f64;
        sy += n.y as f64;
    }
    let (cx, cy) = (sx / tree.nodes.len() as f64, sy / tree.nodes.len() as f64);
    let half = 2600.0f64; // tree units from centre to edge
    let dim = 1600u32;
    let scale = dim as f64 / (2.0 * half);

    let mut canvas = RgbaImage::from_pixel(dim, dim, image::Rgba([5, 8, 12, 255]));

    let proj = |x: f32, y: f32| -> (i64, i64) {
        (
            ((x as f64 - cx) * scale + dim as f64 / 2.0) as i64,
            ((y as f64 - cy) * scale + dim as f64 / 2.0) as i64,
        )
    };

    // Connectors first, under the nodes. Allocated edges glow orange; the rest
    // are a cool grey. Arcs follow their path points; straight edges use the
    // first two.
    for c in &tree.connectors {
        let (x0, y0, x1, y1) = c.bounds;
        if x1 as f64 - cx < -half || x0 as f64 - cx > half {
            continue;
        }
        let _ = (y0, y1);
        let col = match c.state {
            pob_engine::tree::ConnectorState::Active => [0xff, 0xb0, 0x66],
            _ => [0x46, 0x50, 0x5a],
        };
        let pts: Vec<(i64, i64)> = if c.is_arc {
            c.path.iter().map(|(x, y)| proj(*x, *y)).collect()
        } else {
            c.path[..2].iter().map(|(x, y)| proj(*x, *y)).collect()
        };
        for w in pts.windows(2) {
            line(&mut canvas, w[0], w[1], col);
        }
    }

    let mut drawn = 0;
    for n in &tree.nodes {
        let dx = n.x as f64 - cx;
        let dy = n.y as f64 - cy;
        if dx.abs() > half || dy.abs() > half {
            continue;
        }
        let px = (dx * scale + dim as f64 / 2.0) as i64;
        let py = (dy * scale + dim as f64 / 2.0) as i64;
        let fsize = (n.kind.size() as f64 * scale).max(6.0) as u32;

        // Frame under, icon over.
        if let Some(frame) = &n.frame {
            if let Some(spr) = sprite(&mut atlas, &frame.name) {
                let s = imageops::resize(&spr, fsize, fsize, imageops::FilterType::Triangle);
                imageops::overlay(&mut canvas, &s, px - fsize as i64 / 2, py - fsize as i64 / 2);
            }
        }
        if let Some(icon) = &n.icon {
            if !icon.key.is_empty() {
                if let Some(spr) = sprite(&mut atlas, &icon.key) {
                    let isize = (fsize as f64 * 0.8) as u32;
                    let s = imageops::resize(&spr, isize, isize, imageops::FilterType::Triangle);
                    imageops::overlay(&mut canvas, &s, px - isize as i64 / 2, py - isize as i64 / 2);
                    drawn += 1;
                }
            }
        }
    }

    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../decoded-icons");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join("tree-region.png");
    canvas.save(&path).unwrap();
    println!("composited {drawn} node icons -> {}", path.display());
    assert!(drawn > 50, "expected a populated region; only {drawn} nodes drawn");
}
