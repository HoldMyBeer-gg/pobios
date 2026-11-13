//! The connector art as it is read from PoB2's files.

use pob_core::tree_art::TreeArt;
use std::path::PathBuf;

#[test]
fn line_art_is_read_as_a_band_at_three_sizes() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../PathOfBuilding-PoE2/src")
        .canonicalize()
        .expect("PoE2 checkout missing");
    let mut art = TreeArt::new(&src);
    let ctx = egui::Context::default();
    let files = [
        "Character_orbit_normal0.png".to_owned(),
        "Character_orbit_intermediate0.png".to_owned(),
        "Character_orbit_intermediateactive0.png".to_owned(),
    ];
    let [plain, _, lit] = art.line_art(&ctx, "0_5", "Character", &files).expect("line art").clone();

    // The picture is 29 rows across. The plain line fills 11 of them; the
    // lit one 13, with glow out to the picture's edge.
    assert_eq!((plain.rows, plain.band), (29.0, 11.0));
    assert_eq!((lit.rows, lit.band), (29.0, 13.0));
    // Tan along the edge of a plain line, gold along a lit one's.
    assert_eq!(plain.edge, egui::Color32::from_rgb(94, 88, 70));
    assert_eq!(lit.edge, egui::Color32::from_rgb(255, 215, 131));

    // A band is used until it would be squeezed under 0.7 of its rows.
    let rows = |px: f32| lit.level(px).map(|tex| tex.size()[1]);
    assert_eq!(rows(40.0), Some(29));
    assert_eq!(rows(20.3), Some(29));
    assert_eq!(rows(20.0), Some(14));
    assert_eq!(rows(9.0), Some(7));
    assert_eq!(rows(4.0), None);
}
