//! JPEG 2000 data as PDF/X-4 6.27 judges it: the JPX file format's boxes (ITU-T T.800 Annex I = ISO/IEC 15444-1, and
//! T.801 Annex M = 15444-2) or a bare codestream's SIZ marker (T.800, A.5.1). Nothing is decoded: only the header boxes
//! and the codestream's image and tile size marker are read, and what is wrong with their structure is listed.

/// What the header of JPEG 2000 data says.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Jpx {
    /// Whether the data is in the JPX (or JP2) file format, rather than a bare codestream.
    pub(crate) wrapped: bool,
    /// The number of channels after any palette (`pclr`), else from `ihdr` or the codestream.
    pub(crate) channels: Option<u16>,
    /// The bit depth of each of those channels.
    pub(crate) depths: Vec<u8>,
    /// Each colour specification box (`colr`) in the header.
    pub(crate) colours: Vec<Colour>,
    /// The File Type box's brand and compatibility list.
    pub(crate) brand: Option<[u8; 4]>,
    pub(crate) compatible: Vec<[u8; 4]>,
    /// Whether a Reader Requirements box directly follows the File Type box (T.801, M.11.1).
    pub(crate) requirements: bool,
    /// The top-level boxes, in order.
    pub(crate) order: Vec<[u8; 4]>,
    /// The Image Header box's compression type (`C`).
    pub(crate) compression: Option<u8>,
    /// The codestream's capabilities (`Rsiz`).
    pub(crate) capabilities: Option<u16>,
    /// What is wrong with the structure, in words.
    pub(crate) structure: Vec<&'static str>,
}

/// A colour specification box: method, approximation, and for method 1 the enumerated colour space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Colour {
    pub(crate) method: u8,
    pub(crate) approx: u8,
    pub(crate) enumerated: Option<u32>,
}

/// The JP2 and JPX signature box's content (T.800, I.5.1).
const SIGNATURE: [u8; 4] = [0x0D, 0x0A, 0x87, 0x0A];

/// A box: its type and content.
type Box<'a> = ([u8; 4], &'a [u8]);

/// The boxes in `data`, and whether they cover it exactly.
fn boxes(mut data: &[u8]) -> (Vec<Box<'_>>, bool) {
    let mut out = Vec::new();
    while let Some((length, kind)) = data
        .split_first_chunk::<4>()
        .and_then(|(l, rest)| Some((u32::from_be_bytes(*l), rest.first_chunk::<4>()?)))
    {
        let (header, total) = match length {
            0 => (8, data.len()),
            1 => {
                let Some(large) = data.get(8..16).and_then(|b| <[u8; 8]>::try_from(b).ok()) else {
                    break;
                };
                (16, usize::try_from(u64::from_be_bytes(large)).unwrap_or(usize::MAX))
            }
            n => (8, usize::try_from(n).unwrap_or(usize::MAX)),
        };
        let Some(content) = data.get(header..total).filter(|_| total >= header) else {
            break;
        };
        out.push((*kind, content));
        data = data.get(total..).unwrap_or_default();
    }
    (out, data.is_empty())
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_be_bytes)
}

fn half(bytes: &[u8], at: usize) -> Option<u16> {
    bytes
        .get(at..at + 2)
        .and_then(|b| b.try_into().ok())
        .map(u16::from_be_bytes)
}

/// What a codestream's SIZ marker says: capabilities, width and height, and each component's bit depth.
struct Siz {
    capabilities: u16,
    size: (u32, u32),
    depths: Vec<u8>,
}

fn siz(codestream: &[u8]) -> Option<Siz> {
    if codestream.get(..4)? != [0xFF, 0x4F, 0xFF, 0x51] {
        return None;
    }
    let capabilities = half(codestream, 6)?;
    let (x, y) = (word(codestream, 8)?, word(codestream, 12)?);
    let (x0, y0) = (word(codestream, 16)?, word(codestream, 20)?);
    let count = half(codestream, 40)?;
    let depths = (0..usize::from(count))
        .map(|i| codestream.get(42 + 3 * i).map(|s| (s & 0x7F) + 1))
        .collect::<Option<Vec<u8>>>()?;
    Some(Siz {
        capabilities,
        size: (x.checked_sub(x0)?, y.checked_sub(y0)?),
        depths,
    })
}

/// What the JP2 Header box holds.
#[derive(Default)]
struct Header<'a> {
    ihdr: Option<&'a [u8]>,
    bpcc: Option<Vec<u8>>,
    colours: Vec<Colour>,
    palette: Option<(u16, Vec<u8>)>,
    ihdr_first: bool,
}

fn header(content: &[u8]) -> Header<'_> {
    let (inner, _) = boxes(content);
    let mut out = Header {
        ihdr_first: inner.first().is_some_and(|(k, _)| k == b"ihdr"),
        ..Header::default()
    };
    for (kind, body) in inner {
        match &kind {
            b"ihdr" => out.ihdr = Some(body),
            b"bpcc" => out.bpcc = Some(body.iter().map(|b| (b & 0x7F) + 1).collect()),
            b"colr" => out.colours.push(Colour {
                method: body.first().copied().unwrap_or(0),
                approx: body.get(2).copied().unwrap_or(0),
                enumerated: (body.first() == Some(&1)).then(|| word(body, 3)).flatten(),
            }),
            b"pclr" => {
                let columns = body.get(2).copied().unwrap_or(0);
                let depths = body
                    .iter()
                    .skip(3)
                    .take(usize::from(columns))
                    .map(|b| (b & 0x7F) + 1)
                    .collect();
                out.palette = Some((u16::from(columns), depths));
            }
            _ => {}
        }
    }
    out
}

/// Read the header of JPEG 2000 data; `None` when it is neither the file format nor a codestream.
pub(crate) fn read(data: &[u8]) -> Option<Jpx> {
    if let Some(siz) = siz(data) {
        let channels = u16::try_from(siz.depths.len()).ok();
        return Some(Jpx {
            channels,
            depths: siz.depths,
            capabilities: Some(siz.capabilities),
            ..Jpx::default()
        });
    }
    let (top, whole) = boxes(data);
    let (first, signature) = top.first()?;
    if (first, *signature) != (b"jP  ", SIGNATURE.as_slice()) {
        return None;
    }
    let mut jpx = Jpx {
        wrapped: true,
        order: top.iter().map(|(k, _)| *k).collect(),
        ..Jpx::default()
    };
    let mut structure = Vec::new();
    let mut problem = |p: &'static str, holds: bool| {
        if holds {
            structure.push(p);
        }
    };
    problem("box lengths do not add up to the data", !whole);
    let ftyp = top.get(1).filter(|(k, _)| k == b"ftyp").map(|(_, c)| *c);
    problem("the File Type box is not second", ftyp.is_none());
    jpx.requirements = top.get(2).is_some_and(|(k, _)| k == b"rreq");
    jpx.brand = ftyp.and_then(|f| f.first_chunk::<4>()).copied();
    jpx.compatible = ftyp
        .and_then(|f| f.get(8..))
        .unwrap_or_default()
        .as_chunks::<4>()
        .0
        .to_vec();
    let found = top.iter().find(|(k, _)| k == b"jp2h").map(|(_, c)| header(c));
    problem("no JP2 Header box", found.is_none());
    let head = found.unwrap_or_default();
    problem(
        "the Image Header box is not first in the JP2 Header box",
        !head.ihdr_first,
    );
    problem("no Colour Specification box", head.colours.is_empty());
    let ihdr_channels = head.ihdr.and_then(|h| half(h, 8));
    let bpc = head.ihdr.and_then(|h| h.get(10)).copied();
    jpx.compression = head.ihdr.and_then(|h| h.get(11)).copied();
    problem(
        "bit depths vary but there is no Bits Per Component box",
        bpc == Some(255) && head.bpcc.is_none(),
    );
    problem(
        "a Bits Per Component box though the bit depths are the same",
        bpc.is_some_and(|b| b != 255) && head.bpcc.is_some(),
    );
    let jpx_brand = jpx.brand == Some(*b"jpx ");
    problem(
        "an approximation of 0 in a file of brand jpx",
        jpx_brand && head.colours.iter().any(|c| c.approx == 0),
    );
    jpx.depths = match (ihdr_channels, bpc, head.bpcc) {
        (Some(n), Some(bpc), _) if bpc != 255 => vec![(bpc & 0x7F) + 1; usize::from(n)],
        (_, _, Some(depths)) => depths,
        _ => Vec::new(),
    };
    let codestream = top.iter().find(|(k, _)| k == b"jp2c").map(|(_, c)| *c);
    match codestream.map(siz) {
        None => problem("no Contiguous Codestream box", true),
        Some(None) => problem("the codestream does not start with SOC and SIZ", true),
        Some(Some(siz)) => {
            jpx.capabilities = Some(siz.capabilities);
            let width_height = head.ihdr.and_then(|h| word(h, 4).zip(word(h, 0)));
            problem(
                "the codestream's size differs from the Image Header's",
                width_height.is_some_and(|wh| wh != siz.size),
            );
            let counted = u16::try_from(siz.depths.len()).ok();
            problem(
                "the codestream's components differ from the Image Header's",
                ihdr_channels.is_some_and(|n| Some(n) != counted),
            );
            if jpx.depths.is_empty() {
                jpx.depths = siz.depths;
            }
        }
    }
    jpx.channels = ihdr_channels;
    if let Some((columns, depths)) = head.palette {
        jpx.channels = Some(columns);
        jpx.depths = depths;
    }
    jpx.colours = head.colours;
    jpx.structure = structure;
    Some(jpx)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A box of `kind` holding `content`.
    pub(crate) fn boxed(kind: [u8; 4], content: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(content.len() + 8).unwrap().to_be_bytes().to_vec();
        out.extend(kind);
        out.extend(content);
        out
    }

    /// A codestream's SOC and SIZ, 1 by 1 pixels, for `depths` (bits per channel).
    pub(crate) fn codestream(depths: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, 0x4F, 0xFF, 0x51, 0, 0, 0, 0];
        out.extend([0, 0, 0, 1, 0, 0, 0, 1]);
        out.extend([0; 24]);
        out.extend(u16::try_from(depths.len()).unwrap().to_be_bytes());
        for d in depths {
            out.extend([d - 1, 1, 1]);
        }
        out
    }

    /// A baseline JPX file around a codestream (brand jp2, compatible with jpxb and jpx, a Reader Requirements box),
    /// with `header` boxes in `jp2h`.
    pub(crate) fn file(header: &[Vec<u8>], depths: &[u8]) -> Vec<u8> {
        let mut out = boxed(*b"jP  ", &SIGNATURE);
        out.extend(boxed(*b"ftyp", b"jp2 \0\0\0\0jp2 jpxbjpx "));
        out.extend(boxed(*b"rreq", &[1, 0xFF, 0xFF, 0, 0, 0, 0]));
        out.extend(boxed(*b"jp2h", &header.concat()));
        out.extend(boxed(*b"jp2c", &codestream(depths)));
        out
    }

    /// An Image Header box for a 1 by 1 image.
    pub(crate) fn ihdr(channels: u16, bpc: u8) -> Vec<u8> {
        let mut body = vec![0, 0, 0, 1, 0, 0, 0, 1];
        body.extend(channels.to_be_bytes());
        body.extend([bpc, 7, 0, 0]);
        boxed(*b"ihdr", &body)
    }

    /// An enumerated Colour Specification box, approximation 1.
    pub(crate) fn colr(enumerated: u32) -> Vec<u8> {
        let mut body = vec![1, 0, 1];
        body.extend(enumerated.to_be_bytes());
        boxed(*b"colr", &body)
    }

    #[test]
    fn a_baseline_file() {
        let jpx = read(&file(&[ihdr(3, 7), colr(16)], &[8, 8, 8])).unwrap();
        assert_eq!(
            jpx,
            Jpx {
                wrapped: true,
                channels: Some(3),
                depths: vec![8; 3],
                colours: vec![Colour {
                    method: 1,
                    approx: 1,
                    enumerated: Some(16)
                }],
                brand: Some(*b"jp2 "),
                compatible: vec![*b"jp2 ", *b"jpxb", *b"jpx "],
                requirements: true,
                order: vec![*b"jP  ", *b"ftyp", *b"rreq", *b"jp2h", *b"jp2c"],
                compression: Some(7),
                capabilities: Some(0),
                structure: vec![],
            }
        );
    }

    #[test]
    fn depths_palettes_and_bare_codestreams() {
        let varied = read(&file(
            &[
                ihdr(2, 255),
                boxed(*b"bpcc", &[7, 15]),
                boxed(*b"colr", &[2, 0, 1]),
                boxed(*b"res ", b""),
            ],
            &[8, 16],
        ))
        .unwrap();
        assert_eq!((varied.depths, varied.structure), (vec![8, 16], vec![]));
        let palette = read(&file(
            &[ihdr(1, 7), boxed(*b"pclr", &[0, 2, 4, 7, 7, 7, 7]), colr(12)],
            &[8],
        ))
        .unwrap();
        assert_eq!(
            (palette.channels, palette.depths),
            (Some(4), vec![8; 4]),
            "a palette's channels"
        );
        let from_codestream = read(&file(&[ihdr(1, 255), colr(17)], &[16])).unwrap();
        assert_eq!(from_codestream.depths, [16], "no bpcc box: the codestream's depths");
        let bare = read(&codestream(&[1])).unwrap();
        assert_eq!(
            (bare.wrapped, bare.channels, bare.depths, bare.capabilities),
            (false, Some(1), vec![1], Some(0))
        );
        assert_eq!(read(b"not jpeg 2000"), None);
        assert_eq!(read(b"ab"), None, "shorter than a marker");
        assert_eq!(read(&boxed(*b"ftyp", b"")), None, "no signature box first");
        let mut short = codestream(&[8, 8]);
        short.truncate(45);
        assert_eq!(read(&short), None, "a component cut short");
        for cut in [7, 11, 15, 19, 23, 41] {
            assert_eq!(
                read(codestream(&[8]).get(..cut).unwrap()),
                None,
                "a SIZ cut short at {cut} bytes"
            );
        }
        let mut offset = codestream(&[8]);
        offset.get_mut(16..20).unwrap().copy_from_slice(&[0, 0, 0, 9]);
        assert_eq!(read(&offset), None, "an origin past the image");
        let mut low = codestream(&[8]);
        low.get_mut(20..24).unwrap().copy_from_slice(&[0, 0, 0, 9]);
        assert_eq!(read(&low), None, "a vertical origin past the image");
    }

    #[test]
    fn structural_problems() {
        let problems = |data: &[u8]| read(data).unwrap().structure;
        let signature = boxed(*b"jP  ", &SIGNATURE);
        assert_eq!(
            problems(&[signature.clone(), vec![0, 0, 0, 99, b'x']].concat()),
            [
                "box lengths do not add up to the data",
                "the File Type box is not second",
                "no JP2 Header box",
                "the Image Header box is not first in the JP2 Header box",
                "no Colour Specification box",
                "no Contiguous Codestream box",
            ]
        );
        let jpx_brand = boxed(*b"ftyp", b"jpx \0\0\0\0jpx ");
        let header_only = [
            signature.clone(),
            jpx_brand,
            boxed(*b"jp2h", &[boxed(*b"colr", &[1, 0, 0]), ihdr(1, 255)].concat()),
        ]
        .concat();
        assert_eq!(
            problems(&header_only),
            [
                "the Image Header box is not first in the JP2 Header box",
                "bit depths vary but there is no Bits Per Component box",
                "an approximation of 0 in a file of brand jpx",
                "no Contiguous Codestream box",
            ]
        );
        let odd = [
            signature.clone(),
            boxed(*b"ftyp", b"jp2 "),
            boxed(*b"jp2h", &[ihdr(2, 7), boxed(*b"bpcc", &[7, 7]), colr(16)].concat()),
            boxed(*b"jp2c", b"junk"),
        ]
        .concat();
        assert_eq!(
            problems(&odd),
            [
                "a Bits Per Component box though the bit depths are the same",
                "the codestream does not start with SOC and SIZ"
            ]
        );
        let mut tall = ihdr(1, 7);
        tall[11] = 2;
        let mismatch = [
            signature.clone(),
            boxed(*b"ftyp", b"jp2 "),
            boxed(*b"jp2h", &[tall, colr(17)].concat()),
            boxed(*b"jp2c", &codestream(&[8, 8])),
        ]
        .concat();
        assert_eq!(
            problems(&mismatch),
            [
                "the codestream's size differs from the Image Header's",
                "the codestream's components differ from the Image Header's"
            ]
        );
        let bare_header = [
            signature,
            boxed(*b"ftyp", &[]),
            boxed(*b"jp2h", &[boxed(*b"ihdr", &[]), boxed(*b"colr", &[])].concat()),
        ]
        .concat();
        let jpx = read(&bare_header).unwrap();
        assert_eq!(
            (jpx.colours, jpx.brand),
            (
                vec![Colour {
                    method: 0,
                    approx: 0,
                    enumerated: None
                }],
                None
            )
        );
    }

    #[test]
    fn box_lengths() {
        let mut large = 1u32.to_be_bytes().to_vec();
        large.extend(b"jP  ");
        large.extend(20u64.to_be_bytes());
        large.extend(SIGNATURE);
        let mut to_end = 0u32.to_be_bytes().to_vec();
        to_end.extend(b"jp2c");
        to_end.extend(codestream(&[8]));
        assert_eq!(
            read(&[large.clone(), to_end].concat()).unwrap().depths,
            [8],
            "a 64-bit length, then one to the end"
        );
        assert_eq!(boxes(large.get(..12).unwrap()).0, [], "a 64-bit length cut short");
        assert_eq!(
            boxes(&[0, 0, 0, 4, b'a', b'b', b'c', b'd']).0,
            [],
            "a length shorter than the header"
        );
        assert_eq!(boxes(&[0, 0, 0, 8, b'a']).0, [], "a box header cut short");
    }
}
