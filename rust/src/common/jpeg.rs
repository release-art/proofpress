//! JPEG data (ITU-T T.81 = ISO/IEC 10918-1) read at the marker level, as Annex B lays it out: a start-of-image marker,
//! marker segments that fit their lengths, a frame header before the first scan, and an end-of-image marker. The
//! entropy-coded data of a scan is skipped, never decoded.

/// What keeps `data` from being a JPEG at the marker level (T.81 B.1 and B.2), in words, or `None` when nothing does.
pub(crate) fn problem(data: &[u8]) -> Option<String> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return Some("no start-of-image marker at its start".to_owned());
    }
    let mut at = 2;
    let mut frame = false;
    let mut scan = false;
    loop {
        // Fill bytes (0xFF) may come before a marker (B.1.1.2).
        while data.get(at..at + 2) == Some(&[0xFF, 0xFF]) {
            at += 1;
        }
        let Some(&[first, marker]) = data.get(at..at + 2) else {
            return Some("it ends before its end-of-image marker".to_owned());
        };
        if first != 0xFF {
            return Some(format!("byte {at} is not a marker"));
        }
        match marker {
            0xD9 if scan => return None,
            0xD9 => return Some("it ends before any scan".to_owned()),
            0x01 => at += 2,
            0xD8 => return Some("a second start-of-image marker".to_owned()),
            0xD0..=0xD7 => return Some("a restart marker outside a scan".to_owned()),
            0x00 | 0x02..=0xBF | 0xC8 | 0xF0..=0xFD => return Some(format!("the reserved marker 0xFF{marker:02X}")),
            _ => {
                let Some(&[high, low]) = data.get(at + 2..at + 4) else {
                    return Some("it ends inside a marker segment".to_owned());
                };
                let len = usize::from(u16::from_be_bytes([high, low]));
                let Some(body) = data.get(at + 4..at + 2 + len.max(2)) else {
                    return Some(format!("the 0xFF{marker:02X} segment runs past the end"));
                };
                if let Some(problem) = segment(marker, len, body, frame) {
                    return Some(problem);
                }
                frame |= is_frame(marker);
                at += 2 + len;
                if marker == 0xDA {
                    scan = true;
                    // The entropy-coded data ends at the first marker that is not a stuffed zero or a restart (B.1.1.5).
                    let Some(end) = data.get(at..).and_then(|rest| {
                        rest.windows(2)
                            .position(|w| matches!(w, [0xFF, b] if !matches!(b, 0x00 | 0xD0..=0xD7 | 0xFF)))
                    }) else {
                        return Some("it ends inside a scan".to_owned());
                    };
                    at += end;
                }
            }
        }
    }
}

/// The start-of-frame markers, one per coding process (T.81 Table B.1).
fn is_frame(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
}

/// What is wrong with one marker segment of length `len` and contents `body`, when it is a frame or scan header.
fn segment(marker: u8, len: usize, body: &[u8], frame: bool) -> Option<String> {
    if len < 2 {
        return Some(format!("the 0xFF{marker:02X} segment has a length of {len}"));
    }
    if is_frame(marker) {
        let [precision, _, _, x_high, x_low, components, ..] = *body else {
            return Some("a frame header too short to read".to_owned());
        };
        let lossless = matches!(marker, 0xC3 | 0xC7 | 0xCB | 0xCF);
        let progressive = matches!(marker, 0xC2 | 0xC6 | 0xCA | 0xCE);
        let precisions: &[u8] = match marker {
            0xC0 => &[8],
            _ if lossless => &[2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            _ => &[8, 12],
        };
        return if !precisions.contains(&precision) {
            Some(format!(
                "a sample precision of {precision} for frame type 0xFF{marker:02X}"
            ))
        } else if u16::from_be_bytes([x_high, x_low]) == 0 {
            Some("a frame zero samples wide".to_owned())
        } else if components == 0 || (progressive && components > 4) {
            Some(format!("{components} components in a frame of type 0xFF{marker:02X}"))
        } else if len != 8 + 3 * usize::from(components) {
            Some(format!("a frame header of length {len} for {components} components"))
        } else {
            None
        };
    }
    if marker == 0xDA {
        let Some(&components) = body.first() else {
            return Some("a scan header too short to read".to_owned());
        };
        return if !frame {
            Some("a scan before any frame header".to_owned())
        } else if !(1..=4).contains(&components) {
            Some(format!("{components} components in a scan"))
        } else if len != 6 + 2 * usize::from(components) {
            Some(format!("a scan header of length {len} for {components} components"))
        } else {
            None
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::problem;
    use crate::common::raster::tests::JPEG;

    fn jpeg(segments: &[&[u8]]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for s in segments {
            out.extend_from_slice(s);
        }
        out
    }

    const SOF: &[u8] = &[0xFF, 0xC0, 0, 11, 8, 0, 1, 0, 1, 1, 1, 0x11, 0];
    const SOS: &[u8] = &[0xFF, 0xDA, 0, 8, 1, 1, 0, 0, 63, 0, 0x12, 0xFF, 0x00, 0xFF, 0xD0, 0x34];
    const EOI: &[u8] = &[0xFF, 0xD9];

    #[test]
    fn well_formed_jpeg_passes() {
        assert_eq!(problem(&JPEG), None);
        assert_eq!(problem(&jpeg(&[&[0xFF, 0xFF, 0xFF, 0x01], SOF, SOS, EOI])), None);
        // A lossless frame at 16 bits; a progressive one at 12 with 4 components.
        let lossless = [0xFF, 0xC3, 0, 11, 16, 0, 1, 0, 1, 1, 1, 0x11, 0];
        assert_eq!(problem(&jpeg(&[&lossless, SOS, EOI])), None);
        let progressive = [
            0xFF, 0xC2, 0, 20, 12, 0, 1, 0, 1, 4, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0, 4, 0x11, 0,
        ];
        assert_eq!(problem(&jpeg(&[&progressive, SOS, EOI])), None);
    }

    #[test]
    fn marker_level_faults_are_named() {
        let cases: [(Vec<u8>, &str); 20] = [
            (b"not a jpeg".to_vec(), "no start-of-image marker at its start"),
            (jpeg(&[SOF, SOS]), "it ends inside a scan"),
            (jpeg(&[SOF]), "it ends before its end-of-image marker"),
            (jpeg(&[SOF, &[0x12]]), "it ends before its end-of-image marker"),
            (jpeg(&[SOF, &[0x12, 0x34]]), "byte 15 is not a marker"),
            (jpeg(&[SOF, EOI]), "it ends before any scan"),
            (jpeg(&[&[0xFF, 0xD8]]), "a second start-of-image marker"),
            (jpeg(&[&[0xFF, 0xD3]]), "a restart marker outside a scan"),
            (jpeg(&[&[0xFF, 0xF7, 0, 2]]), "the reserved marker 0xFFF7"),
            (jpeg(&[&[0xFF, 0xE0, 0]]), "it ends inside a marker segment"),
            (jpeg(&[&[0xFF, 0xE0, 0, 9, 1]]), "the 0xFFE0 segment runs past the end"),
            (jpeg(&[&[0xFF, 0xE0, 0, 1]]), "the 0xFFE0 segment has a length of 1"),
            (jpeg(&[&[0xFF, 0xC0, 0, 4, 8, 0]]), "a frame header too short to read"),
            (
                jpeg(&[&[0xFF, 0xC0, 0, 11, 12, 0, 1, 0, 1, 1, 1, 0x11, 0]]),
                "a sample precision of 12 for frame type 0xFFC0",
            ),
            (
                jpeg(&[&[0xFF, 0xC3, 0, 11, 1, 0, 1, 0, 1, 1, 1, 0x11, 0]]),
                "a sample precision of 1 for frame type 0xFFC3",
            ),
            (
                jpeg(&[&[0xFF, 0xC0, 0, 11, 8, 0, 1, 0, 0, 1, 1, 0x11, 0]]),
                "a frame zero samples wide",
            ),
            (
                jpeg(&[&[0xFF, 0xC2, 0, 8, 8, 0, 1, 0, 1, 5]]),
                "5 components in a frame of type 0xFFC2",
            ),
            (
                jpeg(&[&[0xFF, 0xC1, 0, 12, 8, 0, 1, 0, 1, 1, 1, 0x11, 0, 0]]),
                "a frame header of length 12 for 1 components",
            ),
            (jpeg(&[&[0xFF, 0xDA, 0, 2]]), "a scan header too short to read"),
            (jpeg(&[SOS]), "a scan before any frame header"),
        ];
        for (data, want) in cases {
            assert_eq!(problem(&data).as_deref(), Some(want));
        }
        let zero = [0xFF, 0xC0, 0, 8, 8, 0, 1, 0, 1, 0];
        assert_eq!(
            problem(&jpeg(&[&zero])).as_deref(),
            Some("0 components in a frame of type 0xFFC0")
        );
        let scan = [0xFF, 0xDA, 0, 6, 5, 0, 0, 0];
        assert_eq!(problem(&jpeg(&[SOF, &scan])).as_deref(), Some("5 components in a scan"));
        let scan = [0xFF, 0xDA, 0, 9, 1, 1, 0, 0, 63, 0, 0];
        assert_eq!(
            problem(&jpeg(&[SOF, &scan])).as_deref(),
            Some("a scan header of length 9 for 1 components")
        );
    }
}
