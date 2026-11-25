//! JPEG 2000 data as PDF/X-4 6.27 judges it: the JPX file format's boxes (ISO/IEC 15444-1, Annex I, and 15444-2,
//! Annex M) or a bare codestream's SIZ marker (15444-1, A.5.1). Nothing is decoded; only the header boxes and the
//! codestream's image and tile size marker are read.
//
// ponytail: the JPX baseline (15444-2, M.9.2) and the `rreq` box that declares it are not read: the standard is not held
// yet (x4.jpx.baseline and x4.image.jpx-valid stay todo).

/// What the header of JPEG 2000 data says.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Jpx {
    /// Whether the data is in the JPX (or JP2) file format, rather than a bare codestream.
    pub(crate) wrapped: bool,
    /// The number of channels, from `ihdr` or the codestream.
    pub(crate) channels: Option<u16>,
    /// The bit depth of each channel.
    pub(crate) depths: Vec<u8>,
    /// Each colour specification box (`colr`) in the header: its method and, for method 1, its enumerated colour space.
    pub(crate) colours: Vec<(u8, Option<u32>)>,
}

/// The JP2 and JPX signature box's type and content (15444-1, I.5.1).
const SIGNATURE: (&[u8; 4], &[u8]) = (b"jP  ", &[0x0D, 0x0A, 0x87, 0x0A]);

/// The boxes in `data`, as their type and content; reading stops at the first box that does not fit.
fn boxes(mut data: &[u8]) -> Vec<([u8; 4], &[u8])> {
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
    out
}

/// The channel count and bit depths a codestream's SIZ marker gives.
fn siz(codestream: &[u8]) -> Option<(u16, Vec<u8>)> {
    if codestream.get(..4)? != [0xFF, 0x4F, 0xFF, 0x51] {
        return None;
    }
    let count = u16::from_be_bytes([*codestream.get(40)?, *codestream.get(41)?]);
    let depths = (0..usize::from(count))
        .map(|i| codestream.get(42 + 3 * i).map(|s| (s & 0x7F) + 1))
        .collect::<Option<Vec<u8>>>()?;
    Some((count, depths))
}

/// Read the header of JPEG 2000 data; `None` when it is neither the file format nor a codestream.
pub(crate) fn read(data: &[u8]) -> Option<Jpx> {
    if let Some((channels, depths)) = siz(data) {
        return Some(Jpx {
            wrapped: false,
            channels: Some(channels),
            depths,
            colours: Vec::new(),
        });
    }
    let top = boxes(data);
    let (first, signature) = top.first()?;
    if (first, *signature) != (SIGNATURE.0, SIGNATURE.1) {
        return None;
    }
    let mut jpx = Jpx {
        wrapped: true,
        ..Jpx::default()
    };
    let mut codestream = None;
    for (kind, content) in &top {
        match kind {
            b"jp2h" => {
                for (inner, body) in boxes(content) {
                    match &inner {
                        b"ihdr" => {
                            jpx.channels = body.get(8..10).and_then(|n| n.try_into().ok()).map(u16::from_be_bytes);
                            if let (Some(n), Some(&bpc)) = (jpx.channels, body.get(10))
                                && bpc != 255
                            {
                                jpx.depths = vec![(bpc & 0x7F) + 1; usize::from(n)];
                            }
                        }
                        b"bpcc" => jpx.depths = body.iter().map(|b| (b & 0x7F) + 1).collect(),
                        b"colr" => {
                            let method = body.first().copied().unwrap_or(0);
                            let enumerated = (method == 1)
                                .then(|| body.get(3..7).and_then(|e| e.try_into().ok()).map(u32::from_be_bytes))
                                .flatten();
                            jpx.colours.push((method, enumerated));
                        }
                        _ => {}
                    }
                }
            }
            b"jp2c" => codestream = Some(*content),
            _ => {}
        }
    }
    if let Some((channels, depths)) = codestream.and_then(siz) {
        jpx.channels.get_or_insert(channels);
        if jpx.depths.is_empty() {
            jpx.depths = depths;
        }
    }
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

    /// A codestream's SOC and SIZ for `depths` (bits per channel).
    pub(crate) fn codestream(depths: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, 0x4F, 0xFF, 0x51];
        out.extend([0; 36]);
        out.extend(u16::try_from(depths.len()).unwrap().to_be_bytes());
        for d in depths {
            out.extend([d - 1, 1, 1]);
        }
        out
    }

    /// The JPX file format around a codestream, with `header` boxes in `jp2h`.
    pub(crate) fn file(header: &[Vec<u8>], depths: &[u8]) -> Vec<u8> {
        let mut out = boxed(*b"jP  ", &[0x0D, 0x0A, 0x87, 0x0A]);
        out.extend(boxed(*b"ftyp", b"jpx \0\0\0\0jpx "));
        out.extend(boxed(*b"jp2h", &header.concat()));
        out.extend(boxed(*b"jp2c", &codestream(depths)));
        out
    }

    pub(crate) fn ihdr(channels: u16, bpc: u8) -> Vec<u8> {
        let mut body = vec![0, 0, 0, 1, 0, 0, 0, 1];
        body.extend(channels.to_be_bytes());
        body.extend([bpc, 7, 0, 0]);
        boxed(*b"ihdr", &body)
    }

    pub(crate) fn colr(enumerated: u32) -> Vec<u8> {
        let mut body = vec![1, 0, 0];
        body.extend(enumerated.to_be_bytes());
        boxed(*b"colr", &body)
    }

    #[test]
    fn the_file_format_and_bare_codestreams() {
        let jpx = read(&file(&[ihdr(3, 7), colr(16)], &[8, 8, 8])).unwrap();
        assert_eq!(
            jpx,
            Jpx {
                wrapped: true,
                channels: Some(3),
                depths: vec![8; 3],
                colours: vec![(1, Some(16))]
            }
        );
        let varied = read(&file(
            &[ihdr(2, 255), boxed(*b"bpcc", &[7, 15]), boxed(*b"colr", &[2, 0, 0])],
            &[8, 16],
        ))
        .unwrap();
        assert_eq!((varied.depths, varied.colours), (vec![8, 16], vec![(2, None)]));
        let from_codestream = read(&file(&[ihdr(1, 255)], &[16])).unwrap();
        assert_eq!(from_codestream.depths, [16], "no bpcc box: the codestream's depths");
        let bare = read(&codestream(&[1])).unwrap();
        assert_eq!(
            bare,
            Jpx {
                wrapped: false,
                channels: Some(1),
                depths: vec![1],
                colours: vec![]
            }
        );
        assert_eq!(read(b"not jpeg 2000"), None);
        assert_eq!(read(b"ab"), None, "shorter than a marker");
        assert_eq!(
            read(codestream(&[8]).get(..20).unwrap()),
            None,
            "a SIZ without its component count"
        );
        let no_codestream = [boxed(*b"jP  ", &[0x0D, 0x0A, 0x87, 0x0A]), boxed(*b"jp2h", &ihdr(3, 7))].concat();
        assert_eq!(read(&no_codestream).unwrap().depths, [8; 3], "no codestream box");
        assert_eq!(boxes(&[0, 0, 0, 8, b'a']), [], "a box header cut short");
        assert_eq!(read(&boxed(*b"ftyp", b"")), None, "no signature box first");
        assert_eq!(read(codestream(&[8]).get(..41).unwrap()), None, "a SIZ cut short");
        let mut cut = codestream(&[8, 8]);
        cut.truncate(45);
        assert_eq!(read(&cut), None, "a component cut short");
    }

    #[test]
    fn box_lengths() {
        let mut large = 1u32.to_be_bytes().to_vec();
        large.extend(b"jP  ");
        large.extend(20u64.to_be_bytes());
        large.extend([0x0D, 0x0A, 0x87, 0x0A]);
        let mut to_end = 0u32.to_be_bytes().to_vec();
        to_end.extend(b"jp2c");
        to_end.extend(codestream(&[8]));
        let jpx = read(&[large.clone(), to_end].concat()).unwrap();
        assert_eq!(
            jpx.depths,
            [8],
            "a box with a 64-bit length, then one that runs to the end"
        );
        assert_eq!(boxes(large.get(..12).unwrap()), [], "a 64-bit length cut short");
        assert_eq!(
            boxes(&[0, 0, 0, 4, b'a', b'b', b'c', b'd']),
            [],
            "a length shorter than the header"
        );
        assert_eq!(
            boxes(&[0, 0, 0, 99, b'a', b'b', b'c', b'd']),
            [],
            "a length past the end"
        );
        let colr = boxed(*b"colr", &[1, 0]);
        assert_eq!(
            read(&file(&[colr, boxed(*b"ihdr", &[0; 4]), boxed(*b"xxxx", b"")], &[8]))
                .unwrap()
                .colours,
            [(1, None)]
        );
        assert_eq!(read(&file(&[boxed(*b"colr", &[])], &[8])).unwrap().colours, [(0, None)]);
    }
}
