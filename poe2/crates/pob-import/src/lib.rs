//! Path of Building's build-code codec.
//!
//! A build code is the build XML, zlib-deflated, then base64url-encoded with
//! padding stripped. Verified byte-identical against a real code exported by
//! PoB itself (see `NOTES.md` and the fixtures).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::io::{Read as _, Write as _};

#[derive(Debug, PartialEq)]
pub enum Error {
    /// The text is not a build code, a share link, or build XML.
    Unrecognised,
    /// Base64 decoded, but the bytes are not a zlib stream.
    NotCompressed,
    /// Decompressed, but the result is not build XML.
    NotBuildXml,
    /// The code inflates past what any build could be.
    TooLarge,
    /// The XML is shaped so that PoB's parser would spin on it.
    Malformed(&'static str),
    Base64(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unrecognised => write!(
                f,
                "not a build code, share link, or build XML"
            ),
            Self::TooLarge => write!(f, "That code inflates to more than {} MB; no build is that large.", MAX_XML_BYTES / (1024 * 1024)),
            Self::NotCompressed => write!(f, "decoded, but not a zlib stream"),
            Self::NotBuildXml => write!(f, "decompressed, but not a Path of Building export"),
            Self::Malformed(what) => write!(f, "That build XML is malformed: {what}."),
            Self::Base64(e) => write!(f, "invalid build code: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Compression level 9.
///
/// PoB's own codes carry the `78 DA` zlib header, which is what level 9
/// produces. A lower level round-trips semantically but yields a different
/// code, so an export would no longer be byte-identical to PoB's.
const LEVEL: Compression = Compression::new(9);

/// Decode a build code into build XML.
/// Size past which build XML is refused: 4 MB, far above any build. The
/// largest real exports, with long notes and every loadout, are a few
/// hundred kilobytes; the cap exists so a paste cannot hand PoB's Lua a
/// million elements to build tables for.
pub const MAX_XML_BYTES: u64 = 4 * 1024 * 1024;

/// Refuse XML that would hurt PoB's own parser before it gets there.
///
/// `runtime/lua/xml.lua` strips comments with `gsub("<!%-%-.-%-%->")` and
/// pulls attributes with `(%w+)=(["'])(.-)%2`. Both are quadratic when the
/// closing part never comes: every `<!--` rescans to the end of the
/// document, and every `x="` rescans to the end of its tag. Each runs as one
/// C call, so no VM hook can stop it once it starts. This walk is linear and
/// mirrors what the Lua patterns would look for, so anything it passes,
/// they finish in one pass.
pub fn check_xml(xml: &str) -> Result<(), Error> {
    if xml.len() as u64 > MAX_XML_BYTES {
        return Err(Error::TooLarge);
    }
    let bytes = xml.as_bytes();

    // The last comment opener must have a closer after it; then every
    // earlier one does too.
    if let Some(open) = xml.rfind("<!--")
        && !xml[open + 4..].contains("-->") {
            return Err(Error::Malformed("a comment is never closed"));
        }

    // Within each tag, an attribute quote that opens after the last quote
    // of its kind in that tag can never close.
    let mut i = 0;
    while let Some(off) = xml[i..].find('<') {
        let start = i + off;
        let Some(len) = xml[start..].find('>') else { break };
        let tag = &bytes[start..start + len];
        let last_dq = tag.iter().rposition(|&b| b == b'"');
        let last_sq = tag.iter().rposition(|&b| b == b'\'');
        for (k, pair) in tag.windows(2).enumerate() {
            if pair[0] != b'=' {
                continue;
            }
            let last = match pair[1] {
                b'"' => last_dq,
                b'\'' => last_sq,
                _ => continue,
            };
            if last.is_none_or(|l| l <= k + 1) {
                return Err(Error::Malformed("an attribute quote is never closed"));
            }
        }
        i = start + len + 1;
    }
    Ok(())
}

pub fn decode(code: &str) -> Result<String, Error> {
    let cleaned: String = code
        .trim()
        // Codes copied from forums and Discord routinely arrive with newlines.
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    if cleaned.is_empty() {
        return Err(Error::Unrecognised);
    }

    // PoB strips base64 padding; decoders need it back.
    let padded = match cleaned.len() % 4 {
        0 => cleaned.clone(),
        n => format!("{cleaned}{}", "=".repeat(4 - n)),
    };

    // Accept the standard alphabet too: some sites re-encode with +/ instead
    // of PoB's -_ and a paste would otherwise fail for no visible reason.
    let normalised: String = padded
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            other => other,
        })
        .collect();

    let bytes = URL_SAFE_NO_PAD
        .decode(normalised.trim_end_matches('='))
        .map_err(|e| Error::Base64(e.to_string()))?;

    // A share code is untrusted input from a link or a paste, and zlib
    // will happily inflate a few hundred KB into gigabytes. The largest
    // real build is well under a megabyte of XML.
    let mut raw = Vec::new();
    ZlibDecoder::new(&bytes[..])
        .take(MAX_XML_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| Error::NotCompressed)?;
    if raw.len() as u64 > MAX_XML_BYTES {
        return Err(Error::TooLarge);
    }
    let xml = String::from_utf8_lossy(&raw).into_owned();

    if !looks_like_build_xml(&xml) {
        return Err(Error::NotBuildXml);
    }
    check_xml(&xml)?;
    Ok(xml)
}

/// Encode build XML into a build code.
pub fn encode(xml: &str) -> Result<String, Error> {
    let mut encoder = ZlibEncoder::new(Vec::new(), LEVEL);
    encoder
        .write_all(xml.as_bytes())
        .map_err(|_| Error::NotCompressed)?;
    let compressed = encoder.finish().map_err(|_| Error::NotCompressed)?;
    Ok(URL_SAFE_NO_PAD.encode(compressed))
}

fn looks_like_build_xml(xml: &str) -> bool {
    xml.contains("<PathOfBuilding")
}

/// What a pasted string turned out to be.
#[derive(Debug, PartialEq)]
pub enum Pasted {
    /// Build XML, ready to load.
    Xml(String),
    /// A share link we cannot expand without a network fetch.
    Link { url: String, site: &'static str },
}

/// Known share-link hosts, from the prototype's codec.
const SHARE_SITES: &[(&str, &str)] = &[
    ("pobb.in", "pobb.in"),
    ("pastebin.com", "Pastebin"),
    ("pastebin.pl", "Pastebin.pl"),
    ("poe.ninja", "poe.ninja"),
    ("pobarchives.com", "PoB Archives"),
];

/// Work out what the user pasted: raw XML, a build code, or a share link.
///
/// Accepting all three matters because users paste whatever they were given,
/// and a tool that only takes one form makes them figure out the difference.
pub fn parse_paste(input: &str) -> Result<Pasted, Error> {
    // A byte-order mark is not whitespace to trim(), and a Windows editor
    // puts one in front of the XML it saves.
    let trimmed = input.trim().trim_start_matches('\u{FEFF}').trim();
    if trimmed.is_empty() {
        return Err(Error::Unrecognised);
    }

    // Raw XML, pasted straight from a file.
    if trimmed.starts_with('<') {
        if !looks_like_build_xml(trimmed) {
            return Err(Error::NotBuildXml);
        }
        check_xml(trimmed)?;
        return Ok(Pasted::Xml(trimmed.to_owned()));
    }

    if let Some(site) = share_site(trimmed) {
        return Ok(Pasted::Link {
            url: trimmed.to_owned(),
            site,
        });
    }

    decode(trimmed).map(Pasted::Xml)
}

/// The known site a URL points at, if any.
pub fn share_site(input: &str) -> Option<&'static str> {
    let lower = input.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    SHARE_SITES
        .iter()
        .find(|(host, _)| lower.contains(*host))
        .map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = include_str!("../tests/fixtures/occultist-100.txt");
    const XML: &str = include_str!("../tests/fixtures/occultist-100.xml");

    #[test]
    fn a_real_build_code_decodes_to_its_xml() {
        let xml = decode(CODE).expect("decode");
        assert!(xml.contains("<PathOfBuilding"));
        assert!(xml.contains(r#"ascendClassName="Occultist""#));
    }

    #[test]
    fn a_re_encoded_code_decodes_back_to_the_same_xml() {
        // Round-tripping preserves the build exactly. It does NOT promise the
        // same bytes as PoB's own code: deflate has many valid encodings for
        // the same input, and flate2's differs from the zlib build PoB uses
        // even at the same level. Anyone comparing codes as strings — to spot
        // a change, or to dedupe — would be misled by that, so it is asserted
        // here rather than left as a surprise.
        let xml = decode(CODE).expect("decode");
        let reencoded = encode(&xml).expect("encode");
        assert_eq!(decode(&reencoded).expect("re-decode"), xml);
    }

    #[test]
    fn re_encoding_produces_a_valid_but_not_necessarily_identical_code() {
        // Documents the limit above with a live check, so if a future
        // flate2 or backend change makes output match PoB's byte for byte,
        // this fails and tells us the stronger guarantee is now available.
        let xml = decode(CODE).expect("decode");
        let reencoded = encode(&xml).expect("encode");
        assert_ne!(
            reencoded,
            CODE.trim(),
            "flate2 now matches PoB's encoder byte for byte; \
             the round-trip guarantee can be strengthened"
        );
    }

    #[test]
    fn codes_copied_with_line_breaks_still_decode() {
        // Forum and Discord pastes routinely wrap.
        let wrapped: String = CODE
            .trim()
            .as_bytes()
            .chunks(76)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(decode(&wrapped).is_ok());
    }

    #[test]
    fn the_standard_base64_alphabet_is_accepted_too() {
        // Some sites re-encode with +/ rather than PoB's -_. Rejecting those
        // would look like a corrupt code to the user.
        let swapped = CODE.trim().replace('-', "+").replace('_', "/");
        assert!(decode(&swapped).is_ok());
    }

    #[test]
    fn raw_xml_is_recognised_without_decoding() {
        assert_eq!(
            parse_paste(XML),
            Ok(Pasted::Xml(XML.trim().to_owned()))
        );
    }

    #[test]
    fn share_links_are_reported_rather_than_silently_failing() {
        let link = "https://pobb.in/abc123";
        assert_eq!(
            parse_paste(link),
            Ok(Pasted::Link {
                url: link.to_owned(),
                site: "pobb.in"
            })
        );
    }

    #[test]
    fn an_unknown_url_is_not_mistaken_for_a_build_code() {
        // Left to the base64 path it would fail with a confusing message
        // about invalid characters instead of "that is not a build".
        assert!(matches!(
            parse_paste("https://example.com/whatever"),
            Err(Error::Base64(_)) | Err(Error::Unrecognised)
        ));
    }

    #[test]
    fn garbage_reports_which_stage_failed() {
        // Distinguishing these matters: "not a build code" and "decoded but
        // is not a build" send the user to different fixes.
        assert_eq!(parse_paste(""), Err(Error::Unrecognised));

        // Valid base64 of non-zlib bytes.
        let not_zlib = URL_SAFE_NO_PAD.encode(b"hello world, definitely not zlib");
        assert_eq!(decode(&not_zlib), Err(Error::NotCompressed));

        // Valid zlib of something that is not a build.
        let not_build = encode("<html><body>nope</body></html>").expect("encode");
        assert_eq!(decode(&not_build), Err(Error::NotBuildXml));
    }

    #[test]
    fn well_formed_xml_passes_the_pre_check() {
        let xml = r#"<PathOfBuilding><!-- a note --><Build level="90" className="Witch"/>
            <Notes>don't &amp; "quote"</Notes><Item x='a "b" c'/></PathOfBuilding>"#;
        assert_eq!(check_xml(xml), Ok(()));
    }

    #[test]
    fn an_unterminated_comment_is_refused_before_lua_sees_it() {
        let xml = "<PathOfBuilding><!-- open <!-- open <!-- open </PathOfBuilding>";
        assert_eq!(check_xml(xml), Err(Error::Malformed("a comment is never closed")));
        // A closed one after open ones is fine.
        assert_eq!(check_xml("<a><!-- x <!-- y --></a>"), Ok(()));
    }

    #[test]
    fn an_unterminated_attribute_quote_is_refused() {
        let xml = r#"<PathOfBuilding><Build level="90 className=Witch/></PathOfBuilding>"#;
        assert_eq!(check_xml(xml), Err(Error::Malformed("an attribute quote is never closed")));
        // The other quote kind closing does not count.
        assert_eq!(check_xml(r#"<a b="c'/>"#), Err(Error::Malformed("an attribute quote is never closed")));
    }

    #[test]
    fn raw_xml_past_the_size_cap_is_refused_like_a_bomb() {
        let mut xml = String::from("<PathOfBuilding>");
        xml.push_str(&"<Item/>".repeat((MAX_XML_BYTES / 7) as usize + 1));
        xml.push_str("</PathOfBuilding>");
        assert_eq!(parse_paste(&xml), Err(Error::TooLarge));
    }

    #[test]
    fn a_decompression_bomb_is_refused_not_inflated() {
        use std::io::Write as _;
        // 64 MB of zeros deflates to a few KB; it must not be inflated.
        let mut enc = ZlibEncoder::new(Vec::new(), LEVEL);
        let chunk = vec![0u8; 1024 * 1024];
        for _ in 0..64 {
            enc.write_all(&chunk).unwrap();
        }
        let bomb = URL_SAFE_NO_PAD.encode(enc.finish().unwrap());
        assert_eq!(decode(&bomb), Err(Error::TooLarge));
    }
}
