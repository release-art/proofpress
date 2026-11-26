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
    Some(computed_id(profile))
}

/// The MD5 of a profile with its flags, rendering intent and Profile ID fields set to zero (7.2.18), whatever its
/// Profile ID says.
pub(crate) fn computed_id(profile: &[u8]) -> [u8; 16] {
    let mut copy = profile.to_vec();
    for range in [44..48, 64..68, 84..100] {
        copy.iter_mut().skip(range.start).take(range.len()).for_each(|b| *b = 0);
    }
    md5::compute(&copy).0
}

/// A profile's description (`desc` tag, ICC.1:2022 9.2.41) as `LittleCMS` gives it for English, US: a v2 text
/// description's ASCII part, a multi-localized Unicode record (en-US, else the first English one, else the first), or
/// plain text, each up to its first NUL. `None` when the profile has no readable description.
pub(crate) fn description(profile: &[u8]) -> Option<String> {
    let tag = desc_tag(profile)?;
    match tag.first_chunk::<4>()? {
        b"desc" => {
            let [_, _, _, _, _, _, _, _, l0, l1, l2, l3, text @ ..] = tag else {
                return None;
            };
            Some(ascii(text.get(..length([*l0, *l1, *l2, *l3]))?))
        }
        b"text" => Some(ascii(tag.get(8..)?)),
        b"mluc" => {
            let records = length(*tag.get(8..)?.first_chunk::<4>()?);
            let language = |i: usize| tag.get(16 + 12 * i..).and_then(|r| r.get(..4));
            let chosen = (0..records)
                .find(|&i| language(i) == Some(b"enUS"))
                .or_else(|| (0..records).find(|&i| language(i).and_then(|l| l.get(..2)) == Some(b"en")))
                .unwrap_or(0);
            mluc_record(tag, chosen)
        }
        _ => None,
    }
}

/// Every description the profile gives: each record of a multi-localized one, or the one [`description`] reads.
pub(crate) fn descriptions(profile: &[u8]) -> Vec<String> {
    match desc_tag(profile) {
        Some(tag) if tag.starts_with(b"mluc") => {
            let records = tag
                .get(8..)
                .and_then(|t| t.first_chunk::<4>())
                .map_or(0, |r| length(*r));
            (0..records).map_while(|i| mluc_record(tag, i)).collect()
        }
        _ => description(profile).into_iter().collect(),
    }
}

/// The colorant names of a profile's `colorantTableTag` (`clrt`, ISO 15076-1 9.2.14), in order, each up to its first
/// NUL; `None` when the profile has no such tag.
pub(crate) fn colorant_names(profile: &[u8]) -> Option<Vec<String>> {
    let tag = tag(profile, *b"clrt")?;
    let count = tag.get(8..).and_then(<[u8]>::first_chunk::<4>).map(|c| length(*c))?;
    (0..count)
        .map(|i| tag.get(12 + 38 * i..).and_then(|e| e.get(..32)).map(ascii))
        .collect()
}

/// The `desc` tag's data, when the profile has a header and the tag.
fn desc_tag(profile: &[u8]) -> Option<&[u8]> {
    tag(profile, *b"desc")
}

/// The data of the tag `signature`, when the profile has a header and the tag.
fn tag(profile: &[u8], signature: [u8; 4]) -> Option<&[u8]> {
    header(profile)?;
    let word = |at: usize| profile.get(at..).and_then(<[u8]>::first_chunk::<4>).map(|w| length(*w));
    let count = word(128)?;
    (0..count).find_map(|i| {
        let entry = 132 + 12 * i;
        (*profile.get(entry..)?.first_chunk::<4>()? == signature).then_some(())?;
        profile.get(word(entry + 4)?..)?.get(..word(entry + 8)?)
    })
}

/// Record `i` of a multi-localized Unicode tag, UTF-16 up to its first NUL.
fn mluc_record(tag: &[u8], i: usize) -> Option<String> {
    let [_, _, _, _, l0, l1, l2, l3, o0, o1, o2, o3] = *tag.get(16 + 12 * i..)?.first_chunk::<12>()?;
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

fn ascii(bytes: &[u8]) -> String {
    bytes.iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect()
}

fn length(four: [u8; 4]) -> usize {
    usize::try_from(u32::from_be_bytes(four)).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A profile header followed by `tags`, a tag table and its data, as written.
    fn headed(tags: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 128];
        out[36..40].copy_from_slice(b"acsp");
        out.extend(tags);
        out
    }

    #[test]
    fn every_record_of_a_multi_localized_description() {
        // One mluc tag at 144: two records, "ab" in de-DE and "cd" in en-US.
        let mut tag = b"mluc\0\0\0\0".to_vec();
        tag.extend(2u32.to_be_bytes());
        tag.extend(12u32.to_be_bytes());
        tag.extend(b"deDE");
        tag.extend(4u32.to_be_bytes());
        tag.extend(40u32.to_be_bytes());
        tag.extend(b"enUS");
        tag.extend(4u32.to_be_bytes());
        tag.extend(44u32.to_be_bytes());
        tag.extend(b"\0a\0b\0c\0d");
        let mut table = 1u32.to_be_bytes().to_vec();
        table.extend(b"desc");
        table.extend(144u32.to_be_bytes());
        table.extend(u32::try_from(tag.len()).unwrap().to_be_bytes());
        table.extend(&tag);
        let profile = headed(&table);
        assert_eq!(descriptions(&profile), ["ab", "cd"]);
        assert_eq!(description(&profile).as_deref(), Some("cd"));
        // A record count beyond the records stops at the last one there is.
        let mut short = profile.clone();
        short[144 + 11] = 9;
        assert_eq!(descriptions(&short), ["ab", "cd"]);
        short.truncate(144 + 9);
        assert_eq!(descriptions(&short), Vec::<String>::new());
    }

    #[test]
    fn a_profile_without_a_whole_tag_table_has_no_description() {
        assert_eq!(descriptions(&headed(b"")), Vec::<String>::new());
        assert_eq!(descriptions(&headed(b"\0\0\0\x01desc\0\0")), Vec::<String>::new());
        // The tag's offset is in the file, its size is not.
        assert_eq!(descriptions(&headed(b"\0\0\0\x01desc\0\0\0\x80")), Vec::<String>::new());
        assert_eq!(
            descriptions(&headed(b"\0\0\0\x01desc\0\0\x10\0\0\0\0\x04")),
            Vec::<String>::new()
        );
    }

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

    #[test]
    fn colorant_names_come_from_the_colorant_table_tag() {
        let mut clrt = b"clrt\0\0\0\0\0\0\0\x02".to_vec();
        for name in [b"Cyan".as_slice(), b"Orange"] {
            let mut entry = name.to_vec();
            entry.resize(38, 0);
            clrt.extend(entry);
        }
        let mut table = 1u32.to_be_bytes().to_vec();
        table.extend(b"clrt");
        table.extend(144u32.to_be_bytes());
        table.extend(u32::try_from(clrt.len()).unwrap().to_be_bytes());
        table.extend(&clrt);
        let profile = headed(&table);
        assert_eq!(
            colorant_names(&profile),
            Some(vec!["Cyan".to_owned(), "Orange".to_owned()])
        );
        assert_eq!(colorant_names(&headed(b"")), None);
        assert_eq!(colorant_names(&profile[..144 + 8]), None);
        assert_eq!(colorant_names(&profile[..144 + 30]), None);
        let mut short = 1u32.to_be_bytes().to_vec();
        short.extend(b"clrt");
        short.extend(144u32.to_be_bytes());
        short.extend(8u32.to_be_bytes());
        short.extend(b"clrt\0\0\0\0");
        assert_eq!(colorant_names(&headed(&short)), None);
    }
}
