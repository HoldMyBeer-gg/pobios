//! Turn PoB2 passive-tree art into the `Rgba8` the egui canvas uploads.
//!
//! Two layers: the DDS codec below ([`Dds`], [`decompress`]), and [`Atlas`],
//! which maps a node's icon path through the tree's `ddsCoords` to a sheet
//! file + slice and decodes it, caching each sheet.
//!
//! PoB2 ships every tree texture as a zstd-compressed DX10 `.dds`: BC1 for
//! the icon sheets, BC7 for the backgrounds, usually with a mip chain, and
//! the icon sheets are *texture arrays* (one square slice per icon) rather
//! than pixel-grid atlases. `tree.lua` carries an `assets` table (name ->
//! zoom -> filename) for the whole-image art and a `ddsCoords` table
//! (filename -> sprite-name -> slice index) for the array sheets.
//!
//! Apple GPUs cannot sample BC formats, so the decode has to land on the CPU
//! regardless. That is fine: PoE1 already CPU-decodes png/jpg to `Rgba8` and
//! hands it to egui, and this produces the identical thing.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Which block codec a DDS surface uses. Only the two PoB2 actually ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockFormat {
    /// BC1 (DXT1): 8 bytes per 4x4 block. The icon sheets.
    Bc1,
    /// BC7: 16 bytes per 4x4 block. The backgrounds and portraits.
    Bc7,
}

impl BlockFormat {
    /// Bytes per 4x4 block.
    fn block_bytes(self) -> usize {
        match self {
            BlockFormat::Bc1 => 8,
            BlockFormat::Bc7 => 16,
        }
    }

    fn from_dxgi(dxgi: u32) -> Option<Self> {
        match dxgi {
            70 | 71 | 72 => Some(BlockFormat::Bc1), // TYPELESS / UNORM / UNORM_SRGB
            97 | 98 | 99 => Some(BlockFormat::Bc7),
            _ => None,
        }
    }
}

/// A parsed DDS surface, pointing at its still-compressed block data.
#[derive(Debug)]
pub struct Dds {
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    /// Array slices. 1 for a plain image, N for the icon sheets.
    pub array_size: u32,
    pub format: BlockFormat,
    /// Block bytes for every slice, each slice a full mip chain back to back.
    data: Vec<u8>,
}

/// A decoded image: tightly packed `Rgba8`, ready for an egui texture.
pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba8 {
    /// Shrink by a whole factor, each pixel the average of a block.
    ///
    /// Colour is weighted by alpha, so a clear pixel lends its neighbours
    /// none of whatever colour it happens to hold. Averaged plainly, art with
    /// a soft edge picks up a fringe of that hidden colour.
    pub fn shrunk(&self, factor: u32) -> Rgba8 {
        if factor <= 1 {
            return Rgba8 { width: self.width, height: self.height, pixels: self.pixels.clone() };
        }
        let (w, h) = (self.width / factor, self.height / factor);
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (mut colour, mut alpha) = ([0u64; 3], 0u64);
                for dy in 0..factor {
                    let row = ((y * factor + dy) * self.width + x * factor) as usize * 4;
                    for p in self.pixels[row..row + factor as usize * 4].chunks_exact(4) {
                        let a = p[3] as u64;
                        for c in 0..3 {
                            colour[c] += p[c] as u64 * a;
                        }
                        alpha += a;
                    }
                }
                let count = (factor * factor) as u64;
                for c in colour {
                    pixels.push(if alpha == 0 { 0 } else { ((c + alpha / 2) / alpha) as u8 });
                }
                pixels.push(((alpha + count / 2) / count) as u8);
            }
        }
        Rgba8 { width: w, height: h, pixels }
    }
}

/// zstd magic (little-endian frame), the wrapper on every `.dds.zst`.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Decompress a `.dds.zst` blob, tolerating an already-raw `.dds`.
pub fn decompress(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    if bytes.len() >= 4 && bytes[..4] == ZSTD_MAGIC {
        let mut out = Vec::new();
        zstd::stream::Decoder::new(bytes)?.read_to_end(&mut out)?;
        Ok(out)
    } else {
        Ok(bytes.to_vec())
    }
}

fn u32le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

impl Dds {
    /// Parse a decompressed DDS. Only the DX10 (`DX10` fourCC) path is
    /// exercised because that is all PoB2 ships; the fields we read
    /// (dimensions, mips, DXGI format, array size) live at fixed offsets.
    pub fn parse(dds: Vec<u8>) -> Result<Self, String> {
        if dds.len() < 148 || &dds[0..4] != b"DDS " {
            return Err("not a DDS file".into());
        }
        let height = u32le(&dds, 12);
        let width = u32le(&dds, 16);
        let mip_count = u32le(&dds, 28).max(1);
        let four_cc = &dds[84..88];
        if four_cc != b"DX10" {
            return Err(format!("unexpected DDS fourCC {four_cc:?}, expected DX10"));
        }
        // DX10 extended header sits right after the 128-byte base header.
        let dxgi = u32le(&dds, 128);
        let array_size = u32le(&dds, 140).max(1);
        let format = BlockFormat::from_dxgi(dxgi)
            .ok_or_else(|| format!("unhandled DXGI format {dxgi}"))?;
        let data = dds[148..].to_vec();
        Ok(Dds { width, height, mip_count, array_size, format, data })
    }

    /// Byte length of one mip level at the given dimensions.
    fn mip_bytes(&self, w: u32, h: u32) -> usize {
        let bx = w.div_ceil(4).max(1) as usize;
        let by = h.div_ceil(4).max(1) as usize;
        bx * by * self.format.block_bytes()
    }

    /// Bytes spanning a full mip chain of one array slice.
    fn slice_bytes(&self) -> usize {
        let (mut w, mut h) = (self.width, self.height);
        let mut total = 0;
        for _ in 0..self.mip_count {
            total += self.mip_bytes(w, h);
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        total
    }

    /// Decode slice `index`, mip 0, to `Rgba8`. `index` is the value a
    /// `ddsCoords` entry gives for a sprite; 0 for a plain image.
    pub fn decode_slice(&self, index: u32) -> Result<Rgba8, String> {
        if index >= self.array_size {
            return Err(format!(
                "slice {index} out of range (array_size {})",
                self.array_size
            ));
        }
        let stride = self.slice_bytes();
        let start = index as usize * stride;
        let mip0 = self.mip_bytes(self.width, self.height);
        let block = self
            .data
            .get(start..start + mip0)
            .ok_or("slice data truncated")?;

        let (w, h) = (self.width as usize, self.height as usize);
        let mut argb = vec![0u32; w * h];
        let res = match self.format {
            BlockFormat::Bc1 => texture2ddecoder::decode_bc1(block, w, h, &mut argb),
            BlockFormat::Bc7 => texture2ddecoder::decode_bc7(block, w, h, &mut argb),
        };
        res.map_err(|e| format!("block decode failed: {e}"))?;

        // texture2ddecoder packs each pixel 0xAARRGGBB; unpack to RGBA bytes.
        let mut pixels = Vec::with_capacity(w * h * 4);
        for px in argb {
            pixels.push(((px >> 16) & 0xff) as u8); // R
            pixels.push(((px >> 8) & 0xff) as u8); // G
            pixels.push((px & 0xff) as u8); // B
            pixels.push(((px >> 24) & 0xff) as u8); // A
        }
        Ok(Rgba8 { width: self.width, height: self.height, pixels })
    }
}

/// From a whole `tree.lua`, pull one atlas's `ddsCoords` sprite -> slice map.
///
/// Deliberately a light scan, not a Lua parse: the prototype only needs to
/// confirm the array-slice model and count sprites. The real engine already
/// `dofile`s `tree.lua`, so production reads this table properly there.
pub fn dds_coords_for(tree_lua: &str, filename: &str) -> Vec<(String, u32)> {
    let key = format!("[\"{filename}\"]={{");
    let Some(start) = tree_lua.find(&key) else {
        return Vec::new();
    };
    let body = &tree_lua[start + key.len()..];
    // The block ends at its matching brace; sprite entries never nest, so the
    // first unmatched '}' is ours.
    let end = body.find('}').unwrap_or(body.len());
    let mut out = Vec::new();
    for entry in body[..end].split(',') {
        let entry = entry.trim();
        let Some((name, idx)) = entry.split_once('=') else { continue };
        let name = name.trim().trim_matches(|c| c == '[' || c == ']' || c == '"');
        if let Ok(idx) = idx.trim().parse::<u32>() {
            out.push((name.to_string(), idx));
        }
    }
    out
}

/// The whole `ddsCoords` table, inverted to `icon path -> (sheet file, slice)`.
///
/// Each sheet's block is keyed by its `.dds.zst` filename; we find every such
/// header and read its sprite entries. Slices are returned 0-based, ready for
/// [`Dds::decode_slice`].
pub fn all_dds_coords(tree_lua: &str) -> HashMap<String, (String, u32)> {
    let mut out: HashMap<String, (String, u32)> = HashMap::new();
    // Sheet blocks are headed `["<name>.dds.zst"]={`. Walk each header and
    // reuse the single-sheet reader for its body.
    let needle = ".dds.zst\"]={";
    let mut pos = 0;
    while let Some(rel) = tree_lua[pos..].find(needle) {
        let at = pos + rel;
        // Recover the filename: back up to the opening `["`.
        let head = &tree_lua[..at + ".dds.zst".len()];
        if let Some(open) = head.rfind("[\"") {
            let file = tree_lua[open + 2..at + ".dds.zst".len()].to_string();
            for (sprite, slice) in dds_coords_for(tree_lua, &file) {
                // PoB2's ddsCoords indices are 1-based (they start at 1), while
                // the DDS array slices are 0-based, so the last icon of a sheet
                // otherwise runs off the end and every icon is off by one.
                let slot = (file.clone(), slice.saturating_sub(1));
                // Every skill icon is packed twice, once in a `skills` sheet
                // and once in a `skills-disabled` one. PoB2 greys an icon that
                // is not allocated itself, so take the lit copy.
                match out.get(&sprite) {
                    Some((held, _)) if !held.contains("-disabled") => {}
                    _ => {
                        out.insert(sprite, slot);
                    }
                }
            }
        }
        pos = at + needle.len();
    }
    out
}

/// Resolves and decodes PoB2 tree node icons, caching each decoded sheet.
///
/// Built from a `TreeData/<ver>/` directory and that version's `tree.lua`.
/// Decoding a whole sheet once (they hold ~175 slices) and slicing from the
/// cached `Dds` keeps per-icon cost to one block-decode.
pub struct Atlas {
    dir: PathBuf,
    by_key: HashMap<String, (String, u32)>,
    sheets: HashMap<String, Dds>,
}

impl Atlas {
    /// Build from a tree-data directory and its `tree.lua` text.
    pub fn new(dir: impl AsRef<Path>, tree_lua: &str) -> Self {
        Atlas {
            dir: dir.as_ref().to_path_buf(),
            by_key: all_dds_coords(tree_lua),
            sheets: HashMap::new(),
        }
    }

    /// Number of icon paths the atlas knows how to resolve.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// True when no icon paths resolved (e.g. an empty or unexpected tree).
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// The sheet file and slice an icon path maps to, if known.
    pub fn resolve(&self, key: &str) -> Option<(&str, u32)> {
        self.by_key.get(key).map(|(f, s)| (f.as_str(), *s))
    }

    /// Decode a node icon by its path to `Rgba8`, or `None` if the path is
    /// unknown or the sheet cannot be read.
    pub fn icon_rgba(&mut self, key: &str) -> Option<Rgba8> {
        let (file, slice) = self.by_key.get(key).cloned()?;
        if !self.sheets.contains_key(&file) {
            let raw = std::fs::read(self.dir.join(&file)).ok()?;
            let dds = decompress(&raw).ok().and_then(|d| Dds::parse(d).ok())?;
            self.sheets.insert(file.clone(), dds);
        }
        self.sheets[&file].decode_slice(slice).ok()
    }

    /// Let go of the sheet `key` lives in.
    ///
    /// A sheet is kept after its first use because icons come off it one at a
    /// time for as long as the tree is open. The portrait sheets are not like
    /// that: 80 MB of blocks, wanted a couple of dozen times and then never
    /// again. Reading one after this loads the sheet afresh.
    pub fn release(&mut self, key: &str) {
        if let Some((file, _)) = self.by_key.get(key) {
            self.sheets.remove(file);
        }
    }

    /// How many sheets are being held.
    pub fn held(&self) -> usize {
        self.sheets.len()
    }
}
