//! ICC profile headers (ICC.1:2022, 7.2): the fields the PDF/X rows read. Colour conversion is `lcms2`'s job, in Python.

/// The parts of a profile header PDF/X judges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    /// The profile/device class, such as `prtr` or `mntr` (7.2.5).
    pub(crate) class: [u8; 4],
    /// The data colour space, such as `CMYK` or `RGB ` (7.2.6).
    pub(crate) space: [u8; 4],
    /// The major version and the minor version's first digit (7.2.4).
    pub(crate) version: (u8, u8),
}

/// The header of `profile`, when it is at least a header long and carries the `acsp` signature (7.2.9).
pub(crate) fn header(profile: &[u8]) -> Option<Header> {
    let (head, _) = profile.split_first_chunk::<40>()?;
    let [
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        major,
        minor,
        _,
        _,
        c0,
        c1,
        c2,
        c3,
        s0,
        s1,
        s2,
        s3,
        ..,
        a,
        b,
        c,
        d,
    ] = *head;
    (profile.len() >= 128 && [a, b, c, d] == *b"acsp").then_some(Header {
        class: [c0, c1, c2, c3],
        space: [s0, s1, s2, s3],
        version: (major, minor >> 4),
    })
}

/// A profile's identity for comparing two profiles (PDF/X-4 6.4.3.3): its Profile ID (7.2.18) when that is not zero,
/// otherwise the MD5 of the profile with the flags, rendering intent and Profile ID fields set to zero, as 7.2.18 computes
/// the ID. `None` for data too short to be a profile.
pub(crate) fn identity(profile: &[u8]) -> Option<[u8; 16]> {
    let (head, _) = profile.split_first_chunk::<128>()?;
    let mut id = [0; 16];
    id.iter_mut().zip(head.iter().skip(84)).for_each(|(d, s)| *d = *s);
    if id != [0; 16] {
        return Some(id);
    }
    let mut copy = profile.to_vec();
    for range in [44..48, 64..68, 84..100] {
        copy.iter_mut().skip(range.start).take(range.len()).for_each(|b| *b = 0);
    }
    Some(md5::compute(&copy).0)
}

/// A profile's description (`desc` tag, ICC.1:2022 9.2.41) as `LittleCMS` gives it for English, US: a v2 text
/// description's ASCII part, a multi-localized Unicode record (en-US, else the first English one, else the first), or
/// plain text, each up to its first NUL. `None` when the profile has no readable description.
pub(crate) fn description(profile: &[u8]) -> Option<String> {
    header(profile)?;
    let word = |bytes: &[u8], at: usize| {
        let four: [u8; 4] = bytes.get(at..).unwrap_or_default().first_chunk::<4>().copied()?;
        Some(usize::try_from(u32::from_be_bytes(four)).unwrap_or(usize::MAX))
    };
    let count = word(profile, 128)?;
    let tag = (0..count).find_map(|i| {
        let entry = 132 + 12 * i;
        (profile.get(entry..)?.first_chunk::<4>()? == b"desc").then_some(())?;
        let (start, length) = (word(profile, entry + 4)?, word(profile, entry + 8)?);
        profile.get(start..)?.get(..length)
    })?;
    let ascii = |bytes: &[u8]| {
        bytes
            .iter()
            .take_while(|b| **b != 0)
            .map(|b| char::from(*b))
            .collect::<String>()
    };
    let length = |four: [u8; 4]| usize::try_from(u32::from_be_bytes(four)).unwrap_or(usize::MAX);
    match tag.first_chunk::<4>()? {
        b"desc" => {
            let [_, _, _, _, _, _, _, _, l0, l1, l2, l3, text @ ..] = tag else {
                return None;
            };
            Some(ascii(text.get(..length([*l0, *l1, *l2, *l3]))?))
        }
        b"text" => Some(ascii(tag.get(8..)?)),
        b"mluc" => {
            let records = word(tag, 8)?;
            let record = |i: usize| tag.get(16 + 12 * i..)?.first_chunk::<12>();
            let chosen = (0..records)
                .find(|&i| record(i).is_some_and(|r| r.get(..4) == Some(b"enUS")))
                .or_else(|| (0..records).find(|&i| record(i).is_some_and(|r| r.get(..2) == Some(b"en"))))
                .unwrap_or(0);
            let [_, _, _, _, l0, l1, l2, l3, o0, o1, o2, o3] = *record(chosen)?;
            let units = tag
                .get(length([o0, o1, o2, o3])..)?
                .get(..length([l0, l1, l2, l3]))?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|u| u16::from_be_bytes(*u));
            Some(
                char::decode_utf16(units)
                    .map_while(Result::ok)
                    .take_while(|c| *c != '\0')
                    .collect(),
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_reads_its_fields() {
        let mut profile = vec![0u8; 128];
        profile[8..12].copy_from_slice(&[4, 0x30, 0, 0]);
        profile[12..16].copy_from_slice(b"prtr");
        profile[16..20].copy_from_slice(b"CMYK");
        profile[36..40].copy_from_slice(b"acsp");
        assert_eq!(
            header(&profile),
            Some(Header {
                class: *b"prtr",
                space: *b"CMYK",
                version: (4, 3)
            })
        );
        assert_eq!(header(profile.get(..127).unwrap()), None, "too short");
        assert_eq!(header(profile.get(..39).unwrap()), None, "shorter than the fields");
        profile[36] = b'x';
        assert_eq!(header(&profile), None, "no signature");
    }

    #[test]
    fn identities() {
        let mut profile = vec![7u8; 200];
        assert_eq!(identity(profile.get(..127).unwrap()), None);
        profile.get_mut(84..100).unwrap().fill(0);
        let computed = identity(&profile).unwrap();
        // The flags, rendering intent and ID fields do not count.
        profile.get_mut(44..48).unwrap().fill(1);
        profile.get_mut(64..68).unwrap().fill(2);
        assert_eq!(identity(&profile), Some(computed));
        profile[150] = 9;
        assert_ne!(identity(&profile), Some(computed));
        profile.get_mut(84..100).unwrap().copy_from_slice(&[3; 16]);
        assert_eq!(
            identity(&profile),
            Some([3; 16]),
            "a Profile ID that is set is used as it is"
        );
    }

    /// A profile with the tags `tags` (signature and content), laid out after the tag table.
    fn tagged(tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0u8; 128];
        out[36..40].copy_from_slice(b"acsp");
        out.extend(u32::try_from(tags.len()).unwrap().to_be_bytes());
        let mut at = 132 + 12 * tags.len();
        let mut data: Vec<u8> = Vec::new();
        for (sig, content) in tags {
            out.extend(*sig);
            out.extend(u32::try_from(at).unwrap().to_be_bytes());
            out.extend(u32::try_from(content.len()).unwrap().to_be_bytes());
            at += content.len();
            data.extend(content.iter());
        }
        out.extend(data);
        out
    }

    fn text_description(text: &str) -> Vec<u8> {
        let mut tag = b"desc\0\0\0\0".to_vec();
        tag.extend(u32::try_from(text.len() + 1).unwrap().to_be_bytes());
        tag.extend(text.as_bytes());
        tag.push(0);
        tag
    }

    /// A multi-localized Unicode tag with one record per `(language and country, text)`.
    fn mluc(records: &[(&[u8; 4], &str)]) -> Vec<u8> {
        let mut tag = b"mluc\0\0\0\0".to_vec();
        tag.extend(u32::try_from(records.len()).unwrap().to_be_bytes());
        tag.extend(12u32.to_be_bytes());
        let mut offset = 16 + 12 * records.len();
        let mut strings = Vec::new();
        for (locale, text) in records {
            let units: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
            tag.extend(*locale);
            tag.extend(u32::try_from(units.len()).unwrap().to_be_bytes());
            tag.extend(u32::try_from(offset).unwrap().to_be_bytes());
            offset += units.len();
            strings.extend(units);
        }
        tag.extend(strings);
        tag
    }

    #[test]
    fn descriptions_as_littlecms_gives_them() {
        let describe = |tags: &[(&[u8; 4], Vec<u8>)]| description(&tagged(tags));
        assert_eq!(
            describe(&[(b"cprt", b"x".to_vec()), (b"desc", text_description("Coated FOGRA39"))]).as_deref(),
            Some("Coated FOGRA39")
        );
        let mut text = b"text\0\0\0\0sRGB\0junk".to_vec();
        assert_eq!(describe(&[(b"desc", text.clone())]).as_deref(), Some("sRGB"));
        let records = [(b"frFR", "Français"), (b"enGB", "British"), (b"enUS", "American")];
        assert_eq!(describe(&[(b"desc", mluc(&records))]).as_deref(), Some("American"));
        assert_eq!(describe(&[(b"desc", mluc(&records[..2]))]).as_deref(), Some("British"));
        assert_eq!(describe(&[(b"desc", mluc(&records[..1]))]).as_deref(), Some("Français"));
        assert_eq!(
            describe(&[(b"desc", mluc(&[(b"enUS", "two\0parts")]))]).as_deref(),
            Some("two")
        );
        assert_eq!(
            describe(&[(b"desc", b"XYZ \0\0\0\0".to_vec())]),
            None,
            "an unknown tag type"
        );
        assert_eq!(describe(&[(b"cprt", b"x".to_vec())]), None, "no description");
        text.truncate(3);
        assert_eq!(describe(&[(b"desc", text)]), None, "a tag too short for its type");
        for broken in [
            b"desc\0\0\0\0\0\0\0\x09abc".to_vec(),
            b"desc\0\0\0\0".to_vec(),
            b"text\0\0\0".to_vec(),
        ] {
            assert_eq!(describe(&[(b"desc", broken.clone())]), None, "{broken:?}");
        }
        let mut short_record = mluc(&[(b"enUS", "abc")]);
        short_record.truncate(20);
        assert_eq!(describe(&[(b"desc", short_record)]), None, "a record cut short");
        let mut cut = mluc(&[(b"enUS", "abc")]);
        cut.truncate(30);
        assert_eq!(describe(&[(b"desc", cut)]), None, "a string cut short");
        let mut far = mluc(&[(b"enUS", "abc")]);
        far[24..28].copy_from_slice(&1000u32.to_be_bytes());
        assert_eq!(describe(&[(b"desc", far)]), None, "a string beyond the tag");
        assert_eq!(describe(&[(b"desc", b"mluc\0\0\0".to_vec())]), None, "no record count");
        assert_eq!(
            describe(&[(b"desc", b"mluc\0\0\0\0\0\0".to_vec())]),
            None,
            "no record count"
        );
        let mut table = tagged(&[(b"desc", text_description("x"))]);
        table[140..144].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(description(&table), None, "a tag running beyond the profile");
        table[136..140].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(description(&table), None, "a tag starting beyond the profile");
        let mut two = tagged(&[(b"cprt", b"x".to_vec())]);
        two[128..132].copy_from_slice(&2u32.to_be_bytes());
        assert_eq!(description(&two), None, "a second entry beyond the table");
        two.truncate(143);
        assert_eq!(description(&two), None, "a second entry beyond the profile");
        assert_eq!(
            describe(&[(b"desc", b"mluc\0\0\0\0\0\0\0\0".to_vec())]),
            None,
            "no records"
        );
        table.truncate(140);
        assert_eq!(description(&table), None, "a tag table cut short");
        assert_eq!(description(&table[..130]), None, "no tag count");
        assert_eq!(description(&table[..134]), None, "an entry cut short");
        assert_eq!(description(&table[..138]), None, "an entry without its offset");
        assert_eq!(description(b"not a profile"), None);
    }
}
