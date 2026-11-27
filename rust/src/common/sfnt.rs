//! The table directory of a TrueType or OpenType font program (the `sfnt` wrapper; Apple's TrueType Reference Manual
//! chapter 6 and ISO/IEC 14496-22 4.5): which tables it has and where, and the number of encoding subtables in its
//! `cmap`. Nothing else of the program is read.

/// The bytes of the table `tag` in the font program `data`, `None` when `data` is not a font program or has no such
/// table, or the table runs past the end.
pub(crate) fn table(data: &[u8], tag: [u8; 4]) -> Option<&[u8]> {
    let [_, _, _, _, high, low, ..] = *data else {
        return None;
    };
    (0..usize::from(u16::from_be_bytes([high, low]))).find_map(|i| {
        let [t0, t1, t2, t3, _, _, _, _, o0, o1, o2, o3, l0, l1, l2, l3, ..] = *data.get(12 + 16 * i..)? else {
            return None;
        };
        let offset = u32::from_be_bytes([o0, o1, o2, o3]) as usize;
        let length = u32::from_be_bytes([l0, l1, l2, l3]) as usize;
        ([t0, t1, t2, t3] == tag).then(|| data.get(offset..offset + length))?
    })
}

/// How many encoding subtables the program's `cmap` table lists (its `numTables`), `None` without a readable `cmap`.
pub(crate) fn cmap_subtables(data: &[u8]) -> Option<u16> {
    let [_, _, high, low, ..] = *table(data, *b"cmap")? else {
        return None;
    };
    Some(u16::from_be_bytes([high, low]))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{cmap_subtables, table};

    /// A font program holding `tables`, each placed after the directory in order.
    pub(crate) fn program(tables: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let count = u16::try_from(tables.len()).unwrap();
        let mut out = vec![0, 1, 0, 0];
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + 16 * tables.len();
        let mut body = Vec::new();
        for (tag, data) in tables {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
            out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
            body.extend_from_slice(data);
            offset += data.len();
        }
        out.extend(body);
        out
    }

    #[test]
    fn tables_and_the_cmap_count() {
        let font = program(&[(b"head", &[1, 2]), (b"cmap", &[0, 0, 0, 2])]);
        assert_eq!(table(&font, *b"head"), Some(&[1, 2][..]));
        assert_eq!(table(&font, *b"glyf"), None);
        assert_eq!(cmap_subtables(&font), Some(2));
        // No cmap, a cmap too short to count, a table past the end, a directory cut short, not a font at all.
        assert_eq!(cmap_subtables(&program(&[(b"head", &[0])])), None);
        assert_eq!(cmap_subtables(&program(&[(b"cmap", &[0, 0, 1])])), None);
        let mut past = program(&[(b"cmap", &[0, 0, 0, 1])]);
        past.truncate(past.len() - 1);
        assert_eq!(cmap_subtables(&past), None);
        assert_eq!(cmap_subtables(&font[..20]), None);
        assert_eq!(table(b"abc", *b"cmap"), None);
        assert_eq!(table(&[0, 1, 0, 0, 0, 1], *b"cmap"), None);
    }
}
