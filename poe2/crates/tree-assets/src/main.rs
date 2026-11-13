//! De-risk the PoB2 tree-art pipeline end to end: read the shipped
//! `.dds.zst`, decompress, parse, block-decode to `Rgba8`, and write PNGs so
//! the result is eyeballable.
//!
//! Usage:
//!   decode [TREEDATA_VERSION_DIR] [OUT_DIR]
//! Defaults point at 0_5 and ./decoded.

use std::path::{Path, PathBuf};
use tree_assets::{Dds, dds_coords_for, decompress};

fn main() {
    let mut args = std::env::args().skip(1);
    let tree_dir = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../PathOfBuilding-PoE2/src/TreeData/0_5")
    });
    let out_dir = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("decoded"));
    std::fs::create_dir_all(&out_dir).expect("create out dir");

    let tree_lua = std::fs::read_to_string(tree_dir.join("tree.lua")).unwrap_or_default();

    println!(
        "{:<40} {:>11} {:>9} {:>6} {:>5} {:>10}",
        "file", "dims", "format", "slices", "mips", "raw KiB"
    );
    println!("{}", "-".repeat(88));

    // A BC7 whole-image background, and a BC1 icon-sheet texture array.
    report(&tree_dir, "group-background_92_92_BC7.dds.zst", 0, &out_dir);
    let skills = "skills_64_64_BC1.dds.zst";
    let coords = dds_coords_for(&tree_lua, skills);
    // Decode the first named icon so the PNG is a recognisable sprite.
    let (sprite, slice) = coords.first().cloned().unwrap_or_else(|| ("slice0".into(), 0));
    report(&tree_dir, skills, slice, &out_dir);

    println!();
    println!(
        "ddsCoords for {skills}: {} sprites (confirms the texture-array model).",
        coords.len()
    );
    if !coords.is_empty() {
        println!("  decoded slice {slice} = sprite \"{sprite}\"");
        for (name, idx) in coords.iter().take(4) {
            println!("    {idx:>4}  {name}");
        }
        if coords.len() > 4 {
            println!("    ...  ({} more)", coords.len() - 4);
        }
    }
    println!("\nPNGs written to {}", out_dir.display());
}

fn report(dir: &Path, file: &str, slice: u32, out_dir: &Path) {
    let path = dir.join(file);
    let raw = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            println!("{file:<40}  read failed: {e}");
            return;
        }
    };
    let dds = match decompress(&raw).map_err(|e| e.to_string()).and_then(Dds::parse) {
        Ok(d) => d,
        Err(e) => {
            println!("{file:<40}  {e}");
            return;
        }
    };
    let raw_kib = (dds_raw_len(&raw)) / 1024;
    println!(
        "{:<40} {:>5}x{:<5} {:>9} {:>6} {:>5} {:>10}",
        file,
        dds.width,
        dds.height,
        format!("{:?}", dds.format),
        dds.array_size,
        dds.mip_count,
        raw_kib,
    );

    match dds.decode_slice(slice) {
        Ok(img) => {
            let stem = file.trim_end_matches(".dds.zst");
            let name = if dds.array_size > 1 {
                format!("{stem}.slice{slice}.png")
            } else {
                format!("{stem}.png")
            };
            let ok = image::save_buffer(
                out_dir.join(&name),
                &img.pixels,
                img.width,
                img.height,
                image::ExtendedColorType::Rgba8,
            );
            if let Err(e) = ok {
                println!("  ! PNG write failed for {name}: {e}");
            }
        }
        Err(e) => println!("  ! decode failed: {e}"),
    }
}

fn dds_raw_len(raw: &[u8]) -> usize {
    decompress(raw).map(|d| d.len()).unwrap_or(raw.len())
}
