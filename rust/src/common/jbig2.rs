//! JBIG2 data (ITU-T T.88 = ISO/IEC 14492) read at the segment level, as a `JBIG2Decode` stream holds it: segments in
//! the embedded organisation (PDF 1.6 3.3.6), each a header (T.88 7.2) and as many bytes of data as it says. The
//! region data is skipped, never decoded.

/// The segment types T.88 7.3 defines.
const TYPES: [u8; 21] = [
    0, 4, 6, 7, 16, 20, 22, 23, 36, 38, 39, 40, 42, 43, 48, 49, 50, 51, 52, 53, 62,
];
const IMMEDIATE_GENERIC_REGION: u8 = 38;
const PAGE_INFORMATION: u8 = 48;
/// The file header's identification string (T.88 D.4.1).
const FILE_ID: &[u8] = &[0x97, 0x4A, 0x42, 0x32, 0x0D, 0x0A, 0x1A, 0x0A];

/// What keeps `data` from being JBIG2 segments, in words, or `None` when nothing does. An image's data (`page`) also
/// needs a page information segment; a `/JBIG2Globals` stream holds global segments only and does not.
///
/// PDF says a file header should be removed; one that leads sequential segments is read past, while the random-access
/// organisation, whose headers all come first, cannot be read as embedded segments.
pub(crate) fn problem(data: &[u8], page: bool) -> Option<String> {
    let mut at = 0;
    if data.starts_with(FILE_ID) {
        let Some(&flags) = data.get(FILE_ID.len()) else {
            return Some("it ends inside its file header".to_owned());
        };
        if flags & 1 == 0 {
            return Some("the random-access organisation, not the embedded one".to_owned());
        }
        at = FILE_ID.len() + 1 + if flags & 2 == 0 { 4 } else { 0 };
    }
    let mut pages = false;
    while at < data.len() {
        let (kind, next) = match header(data, at) {
            Ok(segment) => segment,
            Err(problem) => return Some(problem),
        };
        pages |= kind == PAGE_INFORMATION;
        at = next;
    }
    (page && !pages).then(|| "no page information segment".to_owned())
}

/// The type of the segment whose header starts at `at`, and where the next segment starts.
fn header(data: &[u8], at: usize) -> Result<(u8, usize), String> {
    let short = || format!("it ends inside the segment header at byte {at}");
    let Some(&[n0, n1, n2, n3, flags, refs]) = data.get(at..at + 6) else {
        return Err(short());
    };
    let number = u32::from_be_bytes([n0, n1, n2, n3]);
    let kind = flags & 0x3F;
    if !TYPES.contains(&kind) {
        return Err(format!(
            "segment {number} has the type {kind}, which T.88 does not define"
        ));
    }
    // The referred-to segments (7.2.4): a count in the top three bits, or, when they are all set, in 29 bits of a
    // four-byte field followed by one retention bit per segment and one for this segment.
    let (count, mut cursor) = match refs >> 5 {
        7 => {
            let Some(&[_, r1, r2, r3]) = data.get(at + 5..at + 9) else {
                return Err(short());
            };
            let count = u32::from_be_bytes([refs & 0x1F, r1, r2, r3]) as usize;
            (count, at + 9 + (count + 1).div_ceil(8))
        }
        5 | 6 => return Err(format!("segment {number} has an invalid referred-to segment count")),
        count => (usize::from(count), at + 6),
    };
    // Each referred-to number takes 1, 2 or 4 bytes by this segment's number (7.2.5); the page association 1 or 4
    // (7.2.6).
    let width = match number {
        0..=256 => 1,
        257..=65_536 => 2,
        _ => 4,
    };
    cursor += count.saturating_mul(width) + if flags & 0x40 == 0 { 1 } else { 4 };
    let Some(&[l0, l1, l2, l3]) = data.get(cursor..cursor + 4) else {
        return Err(short());
    };
    cursor += 4;
    let length = u32::from_be_bytes([l0, l1, l2, l3]);
    let end = if length == u32::MAX {
        unknown_end(data, cursor, kind)
            .ok_or_else(|| format!("segment {number}'s data of unknown length has no end"))?
    } else {
        cursor + length as usize
    };
    if end > data.len() {
        return Err(format!("segment {number}'s data runs past the end"));
    }
    Ok((kind, end))
}

/// Where the data of an immediate generic region of unknown length ends (7.2.7): after the region information and
/// flags (18 bytes), at the end marker its coding uses (`0xFFAC` for arithmetic coding, `0x0000` for MMR) and a
/// four-byte row count. No other segment type may leave its length unknown.
fn unknown_end(data: &[u8], start: usize, kind: u8) -> Option<usize> {
    if kind != IMMEDIATE_GENERIC_REGION {
        return None;
    }
    let (&flags, rest) = data.get(start + 17..).and_then(<[u8]>::split_first)?;
    let marker: [u8; 2] = if flags & 1 == 1 { [0, 0] } else { [0xFF, 0xAC] };
    let found = rest.windows(2).position(|w| w == marker)?;
    Some(start + 18 + found + 2 + 4)
}

#[cfg(test)]
mod tests {
    use super::problem;

    /// A segment header with a short referred-to field holding `refs` (each 1, 2 or 4 bytes by `number`), page 1 and
    /// data `body`.
    fn segment(number: u32, kind: u8, refs: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = number.to_be_bytes().to_vec();
        out.push(kind);
        let width = match number {
            0..=256 => 1,
            257..=65_536 => 2,
            _ => 4,
        };
        out.push(u8::try_from(refs.len() / width).unwrap_or(0) << 5);
        out.extend_from_slice(refs);
        out.push(1);
        out.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    fn page() -> Vec<u8> {
        segment(1, 48, &[], &[0; 19])
    }

    #[test]
    fn segments_that_fit_pass() {
        assert_eq!(problem(&page(), true), None);
        assert_eq!(problem(&[], false), None);
        // A generic region referring to segment 1, after the page information.
        let image = [page(), segment(2, 38, &[1], &[0; 20]), segment(3, 49, &[], &[])].concat();
        assert_eq!(problem(&image, true), None);
        // Segment 300 refers to two segments with two bytes each; segment 70000 with four, and a 4-byte page.
        let wide = [
            page(),
            segment(300, 0, &[0, 1, 0, 2], &[]),
            segment(70_000, 0, &[0, 0, 0, 1], &[]),
        ]
        .concat();
        assert_eq!(problem(&wide, true), None);
        let mut long_page = 4_u32.to_be_bytes().to_vec();
        long_page.extend_from_slice(&[0x40 | 0x35, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
        assert_eq!(problem(&[page(), long_page].concat(), true), None);
        // The long form: 8 referred-to segments, then two retention bytes.
        let mut long = 5_u32.to_be_bytes().to_vec();
        long.extend_from_slice(&[0, 0xE0, 0, 0, 8, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 1, 0, 0, 0, 0]);
        assert_eq!(problem(&[page(), long].concat(), true), None);
        // A file header, sequential, with and without a page count.
        let header = [0x97, 0x4A, 0x42, 0x32, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(problem(&[&header[..], &[1, 0, 0, 0, 1], &page()].concat(), true), None);
        assert_eq!(problem(&[&header[..], &[3], &page()].concat(), true), None);
    }

    #[test]
    fn unknown_lengths_end_at_their_marker() {
        let unknown = |flags: u8, tail: &[u8]| {
            let mut out = segment(2, 38, &[], &[]);
            out.truncate(out.len() - 4);
            out.extend_from_slice(&[0xFF; 4]);
            out.extend_from_slice(&[0; 17]);
            out.push(flags);
            out.extend_from_slice(tail);
            [page(), out].concat()
        };
        assert_eq!(problem(&unknown(0, &[1, 2, 0xFF, 0xAC, 0, 0, 0, 1]), true), None);
        assert_eq!(problem(&unknown(1, &[1, 2, 0, 0, 0, 0, 0, 1]), true), None);
        let open = Some("segment 2's data of unknown length has no end");
        assert_eq!(problem(&unknown(0, &[1, 2, 3]), true).as_deref(), open);
        let mut other = segment(2, 6, &[], &[]);
        other.truncate(other.len() - 4);
        other.extend_from_slice(&[0xFF; 4]);
        assert_eq!(problem(&[page(), other].concat(), true).as_deref(), open);
        let mut cut = segment(2, 38, &[], &[]);
        cut.truncate(cut.len() - 4);
        cut.extend_from_slice(&[0xFF; 4]);
        assert_eq!(problem(&[page(), cut].concat(), true).as_deref(), open);
    }

    #[test]
    fn segment_faults_are_named() {
        let header = [0x97, 0x4A, 0x42, 0x32, 0x0D, 0x0A, 0x1A, 0x0A];
        let mut runs = page();
        runs.pop();
        let cases: [(Vec<u8>, bool, &str); 8] = [
            (
                page()[..5].to_vec(),
                true,
                "it ends inside the segment header at byte 0",
            ),
            (
                segment(1, 1, &[], &[]),
                true,
                "segment 1 has the type 1, which T.88 does not define",
            ),
            (
                [page(), vec![0, 0, 0, 2, 0, 0xA0]].concat(),
                true,
                "segment 2 has an invalid referred-to segment count",
            ),
            (
                [page(), vec![0, 0, 0, 2, 0, 0xE0, 0]].concat(),
                true,
                "it ends inside the segment header at byte 30",
            ),
            (
                [page(), vec![0, 0, 0, 2, 0, 0, 1, 0]].concat(),
                true,
                "it ends inside the segment header at byte 30",
            ),
            (runs, true, "segment 1's data runs past the end"),
            (segment(1, 0, &[], &[]), true, "no page information segment"),
            (header.to_vec(), true, "it ends inside its file header"),
        ];
        for (data, page, want) in cases {
            assert_eq!(problem(&data, page).as_deref(), Some(want));
        }
        let random = [&header[..], &[0, 0, 0, 0, 1]].concat();
        assert_eq!(
            problem(&random, true).as_deref(),
            Some("the random-access organisation, not the embedded one")
        );
    }
}
