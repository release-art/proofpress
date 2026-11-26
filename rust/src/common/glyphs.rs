//! Which glyph of its font a shown character code selects (ISO 32000-1 9.6.6 and 9.7), and that glyph's advance width
//! in the embedded program, for PDF/X-4 6.5.1 to 6.5.3.
//!
//! The PDF settles some codes alone: a name of `.notdef`, a code a simple font's encoding names nothing for, and CID 0
//! (a code a `CMap` does not map selects it) are `.notdef` whether or not the program is embedded. The rest need the
//! program: a glyph name it has no glyph for, a TrueType code its `cmap` (or, by name, its `post` table) does not map,
//! a GID past its glyphs or a CID its charset lacks is missing; GID 0 through a `CIDToGIDMap` is `.notdef`. A program
//! that cannot be read is reported as such. Codes under a predefined `CMap` are mapped through Adobe's `CMaps`, which
//! `hayro-cmap` bundles. Widths are in thousandths of the font size, as `/Widths` and `/W` give them.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use hayro_cmap::{CMap, CMapName};
use read_fonts::ps::cff::CffFontRef;
use read_fonts::ps::type1::Type1Font;
use read_fonts::ps::{Sid, agl};
use read_fonts::tables::cmap::{Cmap, CmapSubtable, PlatformId};
use read_fonts::types::GlyphId;
use read_fonts::{FontRef, TableProvider};

use super::encoding::{Base, Encoding};
use super::font;
use super::program::{self, Format};
use super::sfnt;
use crate::document::{Dict, Document, Object, Value};

/// What a code selects.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Look {
    /// A glyph of the program, with its advance when the program gives one.
    Glyph(Option<f64>),
    Notdef,
    /// The glyph the code names is not in the program.
    Missing,
    /// The program cannot be read.
    Unreadable,
    /// Only the program could tell, and the font has none.
    Unjudged,
}

/// Each code of `codes`, judged in the font `font`, with the key its PDF width is found by: the code for a simple font,
/// the CID for a composite one (`None` when the `CMap` maps nothing).
pub(crate) fn judge(doc: &Document, font: &Dict, codes: &BTreeSet<u32>) -> BTreeMap<u32, (Look, Option<u32>)> {
    let subtype = font.get(b"Subtype").and_then(Object::name);
    if subtype == Some(b"Type0") {
        return composite(doc, font, codes);
    }
    let encoding = Encoding::of(doc, font);
    let looks: Box<dyn Fn(u8) -> Look> = match subtype {
        Some(b"Type3") => type3(doc, font, encoding),
        Some(b"TrueType") => truetype(doc, font, encoding),
        _ => type1(doc, font, encoding),
    };
    codes
        .iter()
        .map(|&code| {
            let look = u8::try_from(code).map_or(Look::Notdef, &looks);
            (code, (look, Some(code)))
        })
        .collect()
}

fn descriptor<'a>(doc: &'a Document, font: &'a Dict) -> Option<&'a Dict> {
    font.get(b"FontDescriptor")
        .and_then(|d| doc.resolve(d))
        .and_then(Object::dict)
}

/// A name of `.notdef`, or none: `.notdef` whatever the program.
fn named(name: Option<String>) -> Option<String> {
    name.filter(|n| n != ".notdef")
}

/// A simple font whose program is found by glyph name: Type 1, a bare CFF, or the CFF of an OpenType program.
fn type1(doc: &Document, font: &Dict, encoding: Encoding) -> Box<dyn Fn(u8) -> Look> {
    let Some((format, data)) = descriptor(doc, font).and_then(|d| program::of(doc, d)) else {
        // Without the program its built-in encoding is unknown: only a name of `.notdef`, or a code a named base
        // encoding leaves unassigned, is settled.
        return Box::new(move |code| match encoding.name(code, |_| Some(String::new())) {
            Some(name) if name == ".notdef" => Look::Notdef,
            Some(_) => Look::Unjudged,
            None => Look::Notdef,
        });
    };
    let names = match format {
        Format::Type1 => Type1Font::new(&data).ok().map(|font| {
            let own: Vec<Option<String>> = (0..=255)
                .map(|c| font.encoding().and_then(|e| e.glyph_name(c)).map(str::to_owned))
                .collect();
            let glyphs: HashMap<String, Option<f64>> = font
                .glyph_names()
                .map(|(gid, name)| {
                    (
                        name.to_owned(),
                        font.advance(gid, Some(1000.0)).ok().flatten().map(f64::from),
                    )
                })
                .collect();
            (own, glyphs)
        }),
        Format::Cff => cff_names(&data),
        Format::Sfnt => sfnt::table(&data, *b"CFF ").and_then(cff_names),
    };
    let Some((own, glyphs)) = names else {
        return Box::new(|_| Look::Unreadable);
    };
    Box::new(move |code| {
        let builtin = |c: u8| own.get(usize::from(c)).cloned().flatten();
        match named(encoding.name(code, builtin)) {
            None => Look::Notdef,
            Some(name) => glyphs.get(&name).map_or(Look::Missing, |w| Look::Glyph(*w)),
        }
    })
}

/// A bare CFF font's built-in encoding by code and its glyphs by name, with their advances.
type Names = (Vec<Option<String>>, HashMap<String, Option<f64>>);

fn cff_names(data: &[u8]) -> Option<Names> {
    let cff = CffFontRef::new_cff(data, 0, None).ok()?;
    let charset = cff.charset();
    let name = |gid: GlyphId| {
        let sid = charset.as_ref().and_then(|c| c.string_id(gid));
        sid.and_then(|s| cff.string(s))
            .map(|n| String::from_utf8_lossy(n).into_owned())
    };
    let encoding = cff.encoding();
    let own = (0..=255u8)
        .map(|c| encoding.as_ref().and_then(|e| e.map(c)).and_then(name))
        .collect();
    let glyphs = (0..cff.num_glyphs())
        .filter_map(|g| {
            let gid = GlyphId::new(g);
            name(gid).map(|n| (n, cff_advance(&cff, gid)))
        })
        .collect();
    Some((own, glyphs))
}

fn cff_advance(cff: &CffFontRef<'_>, gid: GlyphId) -> Option<f64> {
    let subfont = cff.subfont_index(gid).and_then(|i| cff.subfont(i, &[]).ok());
    let advance = subfont.and_then(|s| cff.advance(&s, gid, &[], Some(1000.0)).ok().flatten());
    advance.map(f64::from)
}

/// A Type 3 font: a glyph is present when `/CharProcs` has its name, and its advance is the `wx` its procedure's `d0` or
/// `d1` sets (9.6.5), through the `/FontMatrix`.
fn type3(doc: &Document, font: &Dict, encoding: Encoding) -> Box<dyn Fn(u8) -> Look> {
    let scale = font
        .get(b"FontMatrix")
        .and_then(|m| doc.resolve(m))
        .and_then(Object::array)
        .and_then(<[Object]>::first)
        .and_then(|n| font::number(doc, n))
        .unwrap_or(0.001)
        * 1000.0;
    let procs: BTreeMap<Vec<u8>, Option<f64>> = font
        .get(b"CharProcs")
        .and_then(|p| doc.resolve(p))
        .and_then(Object::dict)
        .map_or_default(|d| {
            let width = |o: &Object| match o.value {
                Value::Reference(num, _) => doc.stream_data(num).and_then(|data| glyph_width(&data)),
                _ => None,
            };
            d.entries
                .iter()
                .map(|(k, v)| (k.clone(), width(v).map(|w| w * scale)))
                .collect()
        });
    Box::new(move |code| match named(encoding.name(code, |_| None)) {
        None => Look::Notdef,
        Some(name) => procs.get(name.as_bytes()).map_or(Look::Missing, |w| Look::Glyph(*w)),
    })
}

/// The `wx` of a glyph procedure's leading `d0` (`wx wy d0`) or `d1` (`wx wy llx lly urx ury d1`).
fn glyph_width(data: &[u8]) -> Option<f64> {
    let tokens: Vec<&[u8]> = data
        .split(u8::is_ascii_whitespace)
        .filter(|t| !t.is_empty())
        .take(7)
        .collect();
    let d0 = tokens.get(2) == Some(&&b"d0"[..]);
    let d1 = tokens.get(6) == Some(&&b"d1"[..]);
    if !(d0 || d1) {
        return None;
    }
    tokens
        .first()
        .and_then(|t| std::str::from_utf8(t).ok())
        .and_then(|t| t.parse().ok())
}

/// A simple TrueType font, looked up as 9.6.6.4 says: a non-symbolic font's code by its glyph name, through the
/// Unicode (3,1) `cmap`, else the Macintosh Roman (1,0) one, else the `post` table; a symbolic font's code through the
/// (3,0) `cmap` (as itself or in the 0xF000 to 0xF2FF range), else the (1,0) one; with neither, 9.6.6.4 leaves the
/// glyph to the reader and the code is not judged.
fn truetype(doc: &Document, font: &Dict, encoding: Encoding) -> Box<dyn Fn(u8) -> Look> {
    let symbolic = descriptor(doc, font)
        .and_then(|d| d.get(b"Flags"))
        .is_some_and(|f| matches!(f.value, Value::Integer(n) if n & 4 != 0));
    let name = move |code: u8| named(encoding.name(code, |c| Base::Standard.name(c)));
    let Some((_, data)) = descriptor(doc, font).and_then(|d| program::of(doc, d)) else {
        return Box::new(move |code| {
            if symbolic || name(code).is_some() {
                Look::Unjudged
            } else {
                Look::Notdef
            }
        });
    };
    Box::new(move |code| {
        let Ok(program) = FontRef::new(&data) else {
            return Look::Unreadable;
        };
        let gid = if symbolic {
            let ms = subtable(&program, 3, 0);
            let mac = subtable(&program, 1, 0);
            if ms.is_none() && mac.is_none() {
                // 9.6.6.4 leaves the glyph to the reader.
                return Look::Unjudged;
            }
            [0, 0xF000, 0xF100, 0xF200]
                .iter()
                .find_map(|base| ms.as_ref()?.map_codepoint(base + u32::from(code)))
                .or_else(|| mac?.map_codepoint(code))
        } else {
            let Some(glyph) = name(code) else {
                return Look::Notdef;
            };
            let unicode = subtable(&program, 3, 1);
            let mac = subtable(&program, 1, 0);
            unicode
                .zip(agl::name_to_char(&glyph))
                .and_then(|(t, ch)| t.map_codepoint(ch))
                .or_else(|| mac.zip(mac_code(&glyph)).and_then(|(t, c)| t.map_codepoint(c)))
                .or_else(|| post_glyph(&program, &glyph))
        };
        match gid.filter(|g| g.to_u32() != 0) {
            Some(gid) => Look::Glyph(sfnt_advance(&program, gid)),
            None => Look::Missing,
        }
    })
}

fn subtable<'a>(program: &FontRef<'a>, platform: u16, encoding: u16) -> Option<CmapSubtable<'a>> {
    let cmap: Cmap<'a> = program.cmap().ok()?;
    cmap.encoding_records()
        .iter()
        .find(|r| r.platform_id() == PlatformId::new(platform) && r.encoding_id() == encoding)
        .and_then(|r| r.subtable(cmap.offset_data()).ok())
}

/// The Macintosh Roman code of a glyph name, for a (1,0) `cmap`.
fn mac_code(name: &str) -> Option<u8> {
    (0..=255u8).find(|&c| Base::MacRoman.name(c).as_deref() == Some(name))
}

fn post_glyph(program: &FontRef<'_>, name: &str) -> Option<GlyphId> {
    let post = program.post().ok()?;
    (0..program.maxp().ok()?.num_glyphs())
        .find(|&g| post.glyph_name(g.into()) == Some(name))
        .map(GlyphId::from)
}

/// A glyph's advance in thousandths of an em, from `hmtx` and the units per em.
fn sfnt_advance(program: &FontRef<'_>, gid: GlyphId) -> Option<f64> {
    let upem = f64::from(program.head().ok()?.units_per_em());
    let advance = f64::from(program.hmtx().ok().and_then(|h| h.advance(gid))?);
    (upem > 0.0).then(|| advance * 1000.0 / upem)
}

/// A composite font: each code to a CID through its `CMap`, then to a glyph of its descendant `CIDFont`.
fn composite(doc: &Document, font: &Dict, codes: &BTreeSet<u32>) -> BTreeMap<u32, (Look, Option<u32>)> {
    let cmap = cmap(doc, font);
    let cid_font = font
        .get(b"DescendantFonts")
        .and_then(|d| doc.resolve(d))
        .and_then(Object::array)
        .and_then(<[Object]>::first)
        .and_then(|d| doc.resolve(d))
        .and_then(Object::dict);
    let glyph = cid_font.map(|f| cid_glyphs(doc, f));
    codes
        .iter()
        .map(|&code| {
            let Some(cmap) = cmap.as_ref() else {
                return (code, (Look::Unjudged, None));
            };
            let cid = cmap.lookup_cid_code(code, 2).filter(|&c| c != 0);
            let look = match (cid, glyph.as_ref()) {
                (None, _) => Look::Notdef,
                (Some(cid), Some(glyph)) => glyph(cid),
                (Some(_), None) => Look::Unjudged,
            };
            (code, (look, cid))
        })
        .collect()
}

/// The font's `CMap`: `Identity-H` or `-V`, a predefined one by name, or an embedded stream; `None` when it is none of
/// these or cannot be parsed.
fn cmap(doc: &Document, font: &Dict) -> Option<CMap> {
    let encoding = font.get(b"Encoding")?;
    let resolved = doc.resolve(encoding)?;
    let data = match (&resolved.value, &encoding.value) {
        (Value::Name(n), _) if n == b"Identity-H" => return Some(CMap::identity_h()),
        (Value::Name(n), _) if n == b"Identity-V" => return Some(CMap::identity_v()),
        (Value::Name(n), _) => hayro_cmap::load_embedded(CMapName::from_bytes(n))?.to_vec(),
        (_, Value::Reference(num, _)) => doc.stream_data(*num)?,
        _ => return None,
    };
    CMap::parse(&data, hayro_cmap::load_embedded)
}

/// How a descendant `CIDFont` judges a CID.
fn cid_glyphs(doc: &Document, cid_font: &Dict) -> Box<dyn Fn(u32) -> Look> {
    let Some((format, data)) = descriptor(doc, cid_font).and_then(|d| program::of(doc, d)) else {
        return Box::new(|_| Look::Unjudged);
    };
    if cid_font.get(b"Subtype").and_then(Object::name) == Some(b"CIDFontType2") {
        let map = cid_to_gid(doc, cid_font);
        return Box::new(move |cid| {
            let Ok(program) = FontRef::new(&data) else {
                return Look::Unreadable;
            };
            let gid = match &map {
                Some(map) => map
                    .get(usize::try_from(cid).unwrap_or(usize::MAX) * 2..)
                    .and_then(|b| b.first_chunk::<2>())
                    .map_or(0, |b| u32::from(u16::from_be_bytes(*b))),
                None => cid,
            };
            let glyphs = program.maxp().map_or(0, |m| u32::from(m.num_glyphs()));
            match gid {
                0 => Look::Notdef,
                g if g >= glyphs => Look::Missing,
                g => Look::Glyph(sfnt_advance(&program, GlyphId::new(g))),
            }
        });
    }
    let cff_data = match format {
        Format::Sfnt => sfnt::table(&data, *b"CFF ").map(<[u8]>::to_vec),
        _ => Some(data),
    };
    Box::new(move |cid| {
        let Some(cff) = cff_data.as_deref().and_then(|d| CffFontRef::new_cff(d, 0, None).ok()) else {
            return Look::Unreadable;
        };
        let gid = if cff.is_cid() {
            u16::try_from(cid)
                .ok()
                .and_then(|c| cff.charset().and_then(|s| s.glyph_id(Sid::new(c))))
        } else {
            Some(GlyphId::new(cid)).filter(|g| g.to_u32() < cff.num_glyphs())
        };
        gid.map_or(Look::Missing, |g| Look::Glyph(cff_advance(&cff, g)))
    })
}

/// A `CIDFontType2`'s `/CIDToGIDMap` stream, two bytes per CID; `None` for `Identity` or no map.
fn cid_to_gid(doc: &Document, cid_font: &Dict) -> Option<Vec<u8>> {
    match cid_font.get(b"CIDToGIDMap")?.value {
        Value::Reference(num, _) => doc.stream_data(num),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    const TTF: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof.ttf");
    const SYMBOL_TTF: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof-symbol.ttf");
    const MAC_TTF: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof-mac.ttf");
    const CFF_OTF: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof-cff.otf");
    const CFF: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof.cff");
    const CID_OTF: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof-cid.otf");
    const CID_CFF: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof-cid.cff");
    const TYPE1: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof.t1");

    fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    /// The looks of `codes` in font object 10 (`font`), with `more` objects beside it; program streams are 20 to 29.
    fn looks(font: &str, more: &[(u32, Vec<u8>)], codes: &[u32]) -> Vec<Look> {
        let refs: Vec<String> = more.iter().map(|(n, _)| format!("{n} 0 R")).collect();
        let catalog = format!("<< /Font 10 0 R /More [{}] >>", refs.join(" "));
        let mut objects: Vec<(u32, Vec<u8>)> = vec![(1, catalog.into_bytes()), (10, font.as_bytes().to_vec())];
        objects.extend(more.iter().cloned());
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let doc = Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let dict = doc.objects().find(|(n, ..)| *n == 10).unwrap().1.dict().unwrap();
        let judged = judge(&doc, dict, &codes.iter().copied().collect());
        codes.iter().map(|c| judged[c].0.clone()).collect()
    }

    fn glyph(width: f64) -> Look {
        Look::Glyph(Some(width))
    }

    #[test]
    fn type1_and_cff_programs_by_glyph_name() {
        let t1 = (20, stream("/Length1 1 /Length2 1 /Length3 1", TYPE1));
        let file =
            |key: &str, extra: &str| format!("<< /Subtype /Type1 {extra} /FontDescriptor << /{key} 20 0 R >> >>");
        let (a, b, c) = (65, 66, 67);
        assert_eq!(
            looks(&file("FontFile", ""), std::slice::from_ref(&t1), &[a, b, c, 300]),
            [glyph(600.0), glyph(700.0), Look::Notdef, Look::Notdef]
        );
        let differences = "/Encoding << /Differences [67 /C] >>";
        assert_eq!(looks(&file("FontFile", differences), &[t1], &[c]), [Look::Missing]);
        let cff = (20, stream("/Subtype /Type1C", CFF));
        let win = "/Encoding /WinAnsiEncoding";
        assert_eq!(
            looks(&file("FontFile3", ""), std::slice::from_ref(&cff), &[a, 0]),
            [glyph(600.0), Look::Notdef]
        );
        assert_eq!(
            looks(&file("FontFile3", win), &[cff], &[b, c]),
            [glyph(700.0), Look::Missing]
        );
        let otf = (20, stream("/Subtype /OpenType", CFF_OTF));
        assert_eq!(looks(&file("FontFile3", win), &[otf], &[a]), [glyph(600.0)]);
        for unreadable in [(20, stream("", b"junk")), (20, stream("/Subtype /OpenType", TTF))] {
            let key = if unreadable.1.starts_with(b"<<  /Length") {
                "FontFile"
            } else {
                "FontFile3"
            };
            assert_eq!(looks(&file(key, win), &[unreadable], &[a]), [Look::Unreadable]);
        }
        assert_eq!(
            looks(&file("FontFile3", win), &[(20, stream("", CFF))], &[a]),
            [Look::Unjudged]
        );
        let unembedded = |extra: &str| format!("<< /Subtype /Type1 {extra} >>");
        assert_eq!(looks(&unembedded(win), &[], &[a, 0x10]), [Look::Unjudged, Look::Notdef]);
        assert_eq!(
            looks(&unembedded("/Encoding << /Differences [65 /.notdef] >>"), &[], &[a, b]),
            [Look::Notdef, Look::Unjudged]
        );
    }

    #[test]
    fn truetype_programs_by_cmap() {
        let font = |flags: i64, extra: &str| {
            format!("<< /Subtype /TrueType {extra} /FontDescriptor << /Flags {flags} /FontFile2 20 0 R >> >>")
        };
        let program = |data: &[u8]| vec![(20, stream("", data))];
        let win = "/Encoding /WinAnsiEncoding";
        let a = 65;
        assert_eq!(
            looks(&font(32, win), &program(TTF), &[a, 0x10]),
            [glyph(600.0), Look::Notdef]
        );
        let odd = "/Encoding << /BaseEncoding /WinAnsiEncoding /Differences [66 /notaglyph 67 /uni0043] >>";
        assert_eq!(looks(&font(32, odd), &program(TTF), &[66]), [Look::Missing]);
        assert_eq!(looks(&font(32, win), &program(MAC_TTF), &[a]), [glyph(600.0)]);
        // No (3,1) or (1,0) subtable: by name through the post table.
        assert_eq!(
            looks(&font(32, odd), &program(SYMBOL_TTF), &[a, 67]),
            [Look::Missing, glyph(600.0)]
        );
        assert_eq!(
            looks(&font(4, ""), &program(SYMBOL_TTF), &[a, 0x10]),
            [glyph(600.0), Look::Missing]
        );
        assert_eq!(looks(&font(4, ""), &program(MAC_TTF), &[a]), [glyph(600.0)]);
        assert_eq!(looks(&font(4, ""), &program(b"junk"), &[a]), [Look::Unreadable]);
        assert_eq!(looks(&font(4, ""), &program(TTF), &[a]), [Look::Unjudged]);
        let bare = |flags: i64| format!("<< /Subtype /TrueType /FontDescriptor << /Flags {flags} >> >>");
        assert_eq!(looks(&bare(4), &[], &[a]), [Look::Unjudged]);
        assert_eq!(looks(&bare(32), &[], &[a, 0]), [Look::Unjudged, Look::Notdef]);
    }

    /// Our TrueType font without the tables `drop`.
    fn without(drop: &[&[u8; 4]]) -> Vec<u8> {
        let count = usize::from(u16::from_be_bytes([TTF[4], TTF[5]]));
        let tags: Vec<[u8; 4]> = (0..count)
            .map(|i| TTF[12 + 16 * i..16 + 16 * i].try_into().unwrap())
            .collect();
        let kept: Vec<([u8; 4], &[u8])> = tags
            .into_iter()
            .filter(|t| !drop.contains(&t))
            .map(|t| (t, sfnt::table(TTF, t).unwrap()))
            .collect();
        let refs: Vec<(&[u8; 4], &[u8])> = kept.iter().map(|(t, d)| (t, *d)).collect();
        sfnt::tests::program(&refs)
    }

    #[test]
    fn programs_missing_tables_or_broken() {
        let font = |extra: &str| {
            format!(
                "<< /Subtype /TrueType /Encoding << /Differences [65 /uni0041] >> {extra} \
                 /FontDescriptor << /Flags 32 /FontFile2 20 0 R >> >>"
            )
        };
        let program = |data: Vec<u8>| vec![(20, stream("", &data))];
        assert_eq!(looks(&font(""), &program(without(&[b"cmap"])), &[65]), [glyph(600.0)]);
        assert_eq!(
            looks(&font(""), &program(without(&[b"cmap", b"post"])), &[65]),
            [Look::Missing]
        );
        assert_eq!(
            looks(&font(""), &program(without(&[b"cmap", b"maxp"])), &[65]),
            [Look::Missing]
        );
        assert_eq!(
            looks(&font(""), &program(without(&[b"head"])), &[65]),
            [Look::Glyph(None)]
        );
        assert_eq!(
            looks(&font(""), &program(without(&[b"hmtx"])), &[65]),
            [Look::Glyph(None)]
        );
        let type1 =
            |file: &str| format!("<< /Subtype /Type1 /Encoding /WinAnsiEncoding /FontDescriptor << {file} >> >>");
        let junk_cff = vec![(20, stream("/Subtype /Type1C", b"junk"))];
        assert_eq!(looks(&type1("/FontFile3 20 0 R"), &junk_cff, &[65]), [Look::Unreadable]);
        let foo = vec![(20, stream("/Subtype /Foo", CFF))];
        assert_eq!(looks(&type1("/FontFile3 20 0 R"), &foo, &[65]), [Look::Unjudged]);
        assert_eq!(looks(&type1("/FontFile 5"), &[], &[65]), [Look::Unjudged]);
        let not_stream = vec![(20, b"<< >>".to_vec())];
        assert_eq!(looks(&type1("/FontFile 20 0 R"), &not_stream, &[65]), [Look::Unjudged]);
        let type0 = |encoding: &str| {
            format!(
                "<< /Subtype /Type0 /Encoding {encoding} /DescendantFonts [<< /Subtype /CIDFontType2 \
                     /CIDToGIDMap /Identity /FontDescriptor << /FontFile2 20 0 R >> >>] >>"
            )
        };
        let ttf = vec![(20, stream("", TTF)), (21, b"<< >>".to_vec())];
        assert_eq!(looks(&type0("/Identity-H"), &ttf, &[34]), [glyph(600.0)]);
        assert_eq!(looks(&type0("99 0 R"), &ttf, &[34]), [Look::Unjudged]);
        assert_eq!(looks(&type0("21 0 R"), &ttf, &[34]), [Look::Unjudged]);
    }

    #[test]
    fn type3_glyph_procedures() {
        let font = "<< /Subtype /Type3 /FontMatrix [0.002 0 0 0.002 0 0] /Encoding << /Differences [65 /A /B /C /D /E] >> \
            /CharProcs << /A 20 0 R /B 21 0 R /C 22 0 R /D 5 >> >>";
        let procs = vec![
            (20, stream("", b"300 0 d0")),
            (21, stream("", b"350 0 0 0 10 10 d1 0 0 m")),
            (22, stream("", b"q Q")),
        ];
        assert_eq!(
            looks(font, &procs, &[65, 66, 67, 68, 69, 70]),
            [
                glyph(600.0),
                glyph(700.0),
                Look::Glyph(None),
                Look::Glyph(None),
                Look::Missing,
                Look::Notdef
            ]
        );
        let plain = "<< /Subtype /Type3 /Encoding << /Differences [65 /A] >> /CharProcs << /A 20 0 R >> >>";
        assert_eq!(looks(plain, &[(20, stream("", b"x 0 d0"))], &[65]), [Look::Glyph(None)]);
        assert_eq!(looks("<< /Subtype /Type3 >>", &[], &[65]), [Look::Notdef]);
    }

    #[test]
    fn composite_fonts_through_cmaps() {
        let type0 = |encoding: &str, descendant: &str| {
            format!("<< /Subtype /Type0 /Encoding {encoding} /DescendantFonts [<< {descendant} >>] >>")
        };
        let ttf = (20, stream("", TTF));
        let cid2 = "/Subtype /CIDFontType2 /FontDescriptor << /FontFile2 20 0 R >>";
        // Identity: 'A' is GID 34 of our TrueType font.
        assert_eq!(
            looks(&type0("/Identity-H", cid2), std::slice::from_ref(&ttf), &[34, 0, 500]),
            [glyph(600.0), Look::Notdef, Look::Missing]
        );
        let mapped = format!("{cid2} /CIDToGIDMap 21 0 R");
        let map = (21, stream("", &[0, 0, 0, 34, 0, 0]));
        assert_eq!(
            looks(&type0("/Identity-V", &mapped), &[ttf, map], &[1, 2, 3]),
            [glyph(600.0), Look::Notdef, Look::Notdef]
        );
        let bad = (20, stream("", b"junk"));
        assert_eq!(looks(&type0("/Identity-H", cid2), &[bad], &[1]), [Look::Unreadable]);
        let cid0 =
            |subtype: &str| format!("/Subtype /CIDFontType0 /FontDescriptor << /FontFile3 20 0 R >> /X /{subtype}");
        let cid_cff = (20, stream("/Subtype /CIDFontType0C", CID_CFF));
        assert_eq!(
            looks(&type0("/Identity-H", &cid0("a")), &[cid_cff], &[2, 5, 3, 70_000]),
            [glyph(600.0), glyph(700.0), Look::Missing, Look::Notdef]
        );
        let cid_otf = (20, stream("/Subtype /OpenType", CID_OTF));
        assert_eq!(
            looks(&type0("/Identity-H", &cid0("b")), &[cid_otf], &[5]),
            [glyph(700.0)]
        );
        let plain = (20, stream("/Subtype /CIDFontType0C", CFF));
        assert_eq!(
            looks(&type0("/Identity-H", &cid0("c")), &[plain], &[1, 9]),
            [glyph(600.0), Look::Missing]
        );
        let no_cff = (20, stream("/Subtype /OpenType", TTF));
        assert_eq!(
            looks(&type0("/Identity-H", &cid0("d")), &[no_cff], &[1]),
            [Look::Unreadable]
        );
        let none = "/Subtype /CIDFontType2";
        assert_eq!(
            looks(&type0("/Identity-H", none), &[], &[1, 0]),
            [Look::Unjudged, Look::Notdef]
        );
        assert_eq!(
            looks("<< /Subtype /Type0 /Encoding /Identity-H >>", &[], &[1]),
            [Look::Unjudged]
        );
        // A predefined CMap: UniJIS-UCS2-H maps U+0041 to CID 34; an unknown name and a number give no CMap.
        let ttf = (20, stream("", TTF));
        assert_eq!(looks(&type0("/UniJIS-UCS2-H", cid2), &[ttf], &[0x41]), [glyph(600.0)]);
        assert_eq!(looks(&type0("/NoSuchCMap", none), &[], &[0x41]), [Look::Unjudged]);
        assert_eq!(looks(&type0("5", none), &[], &[0x41]), [Look::Unjudged]);
        assert_eq!(looks("<< /Subtype /Type0 >>", &[], &[1]), [Look::Unjudged]);
        let embedded = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n1 begincidrange <0001> <0010> 34 endcidrange\nendcmap CMapName currentdict /CMap defineresource pop end end";
        let cmap = (21, stream("/Type /CMap", embedded));
        let ttf = (20, stream("", TTF));
        assert_eq!(
            looks(&type0("21 0 R", cid2), &[ttf, cmap], &[1, 0x20]),
            [glyph(600.0), Look::Notdef]
        );
    }
}
