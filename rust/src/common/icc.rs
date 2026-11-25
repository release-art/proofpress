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
}
