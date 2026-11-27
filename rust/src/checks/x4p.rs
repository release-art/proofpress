//! PDF/X-4p (ISO 15930-7 Annex A): the PDF/X intent refers to an external output profile instead of embedding one
//! (A.2), and a file set is the file with that profile (A.1).
//!
//! The profile is looked for at the `/URLs` that are files on this computer, relative ones beside the PDF; nothing is
//! fetched over a network. The candidate whose MD5 is the `/CheckSum` is the external profile (A.2). The rows that need
//! it are not evaluated when the file is verified alone, or when only a network URL could still hold it.

use std::path::PathBuf;

use super::{Files, Settings, intent};
use crate::common::url::{Place, place};
use crate::common::{icc, text};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Check, Finding, Level, X4P_FILESET_CHECKSUM_MATCHES, X4P_FILESET_FIELDS_MATCH, X4P_FILESET_PROFILE_VALID,
    X4P_OUTPUT_INTENT_DEST_PROFILE_REF, X4P_OUTPUT_INTENT_NO_DEST_PROFILE, X4P_REF_CHECKSUM, X4P_REF_ICC_VERSION,
    X4P_REF_NO_COLORANT_TABLE, X4P_REF_PROFILE_CS, X4P_REF_PROFILE_NAME, X4P_REF_URLS, X4P_REF_URLS_NO_EF_PLATFORM,
    X5_N_3CLR_4CLR_NOT_RGB_CMYK, X5_N_COLORANT_TABLE_COUNT, X5_N_COLORANT_TABLE_MATCHES_PROFILE,
    X5_N_COLORANT_TABLE_PRESENT, X5_N_PROFILE_ISO15076, X5_N_PROFILECS_XCLR, X5_N_URLS_EF_ALLOWED,
};

/// The keys a `/URLs` file specification may not have (A.2).
const FORBIDDEN: [&[u8]; 4] = [b"EF", b"DOS", b"Mac", b"Unix"];

/// The external profile, as far as it could be found.
pub(super) enum External {
    /// The profile whose MD5 is the `/CheckSum`, and where it was.
    Found { path: PathBuf, data: Vec<u8> },
    /// No file on this computer is the profile, for the reason given; `network` when a URL this does not fetch might
    /// still hold it.
    Unresolved { why: String, network: bool },
    /// Why no profile was looked for: the file is verified alone, or the reference does not say what to look for.
    NotLookedFor(String),
}

/// The PDF/X intent's `/DestOutputProfileRef`, when it is a dictionary.
fn reference(doc: &Document) -> Option<&Dict> {
    doc.resolve(intent::the_intent(doc)?.get(b"DestOutputProfileRef")?)?
        .dict()
}

/// The reference's entry `key`, resolved.
fn entry<'a>(doc: &'a Document, reference: &'a Dict, key: &[u8]) -> Option<&'a Value> {
    doc.resolve(reference.get(key)?).map(|o| &o.value)
}

/// The reference's string `key`, as written.
fn string<'a>(doc: &'a Document, reference: &'a Dict, key: &[u8]) -> Option<&'a [u8]> {
    match entry(doc, reference, key)? {
        Value::String(bytes, _) => Some(bytes),
        _ => None,
    }
}

/// The URL each `/URLs` element names: a file specification's `/F` (or `/UF`), or a string.
/// Each `/URLs` element's URL, and the object number of the file its `/EF` embeds, if any (ISO 15930-8 7.2 allows that
/// for PDF/X-5n; at the other levels `x4p.ref.urls-no-ef-platform` fails it).
fn urls<'a>(doc: &'a Document, reference: &'a Dict) -> Vec<(&'a [u8], Option<u32>)> {
    let items = match entry(doc, reference, b"URLs") {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    let url = |item: &'a Object| match &doc.resolve(item)?.value {
        Value::String(bytes, _) => Some((bytes.as_slice(), None)),
        Value::Dict(dict) => {
            let url = [b"F".as_slice(), b"UF"].iter().find_map(|k| string(doc, dict, k))?;
            let embedded = dict
                .get(b"EF")
                .and_then(|ef| doc.resolve(ef))
                .and_then(Object::dict)
                .and_then(|ef| match ef.get(b"F")?.value {
                    Value::Reference(num, _) => Some(num),
                    _ => None,
                });
            Some((url, embedded))
        }
        _ => None,
    };
    items.iter().filter_map(url).collect()
}

/// The external profile of a PDF/X-4p file, looked for as `settings.files` says.
pub(super) fn external(doc: &Document, settings: &Settings) -> External {
    let Files::Set(folder) = &settings.files else {
        return External::NotLookedFor("the file is verified alone, without its external profile".into());
    };
    let Some(reference) = reference(doc) else {
        return External::NotLookedFor("the PDF/X output intent has no /DestOutputProfileRef".into());
    };
    let Some(checksum) = string(doc, reference, b"CheckSum").filter(|c| c.len() == 16) else {
        return External::NotLookedFor("without a 16-byte /CheckSum no profile can be confirmed".into());
    };
    let mut why = Vec::new();
    let mut network = false;
    for (url, embedded) in urls(doc, reference) {
        let url = String::from_utf8_lossy(url);
        let read = match (embedded, place(&url, folder)) {
            (Some(num), _) => Some((folder.join(format!("{url} (embedded)")), doc.stream_data(num).ok_or(()))),
            (None, Place::Network) => None,
            (None, Place::Local(path)) => {
                let data = std::fs::read(&path).map_err(|_| ());
                Some((path, data))
            }
        };
        match read {
            None => network = true,
            Some((path, data)) => match data {
                Ok(data) if md5::compute(&data).0 == checksum => return External::Found { path, data },
                Ok(data) => why.push(format!(
                    "{url} has MD5 {}, not the /CheckSum",
                    hex(&md5::compute(&data).0)
                )),
                Err(()) => why.push(format!("no file at {url}")),
            },
        }
    }
    if why.is_empty() && !network {
        why.push("/URLs names no profile".into());
    }
    External::Unresolved {
        why: why.join("; "),
        network,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().concat()
}

/// The external profile's data, or why a check that needs it cannot be judged.
pub(super) fn profile(doc: &Document, settings: &Settings) -> Result<(PathBuf, Vec<u8>), String> {
    resolved(external(doc, settings))
}

fn resolved(external: External) -> Result<(PathBuf, Vec<u8>), String> {
    match external {
        External::Found { path, data } => Ok((path, data)),
        External::Unresolved { why, network: true } if why.is_empty() => {
            Err("every URL needs a network, which is not used".into())
        }
        External::Unresolved { why, network: true } => {
            Err(format!("{why}; the other URLs need a network, which is not used"))
        }
        External::Unresolved { why, network: false } => Err(format!("no external profile: {why}")),
        External::NotLookedFor(why) => Err(why),
    }
}

/// Whether `level` is built on PDF/X-4p, so its output profile is external: X-4p itself, X-5pg and X-5n (ISO 15930-8
/// 7.1, 9).
pub(super) fn external_profile_level(level: Level) -> bool {
    matches!(level, Level::X4p | Level::X5pg | Level::X5n)
}

/// The intent's colour space at a level built on PDF/X-4p: the external profile's, else the reference's `/ProfileCS`.
pub(super) fn space(doc: &Document, settings: &Settings) -> Option<[u8; 4]> {
    if !external_profile_level(settings.level) {
        return None;
    }
    match profile(doc, settings) {
        Ok((_, data)) => icc::header(&data).map(|h| h.space),
        Err(_) => string(doc, reference(doc)?, b"ProfileCS")?.first_chunk::<4>().copied(),
    }
}

fn found(check: &'static Check, message: impl Into<String>) -> Finding {
    Finding::new(check, message).citing("A.2")
}

/// A.2: the PDF/X intent refers to its profile with a `/DestOutputProfileRef` dictionary.
pub(super) fn dest_profile_ref(doc: &Document, _: &Settings) -> Vec<Finding> {
    let Some(the_intent) = intent::the_intent(doc) else {
        return Vec::new();
    };
    let message = match the_intent.get(b"DestOutputProfileRef") {
        None => "the PDF/X output intent has no /DestOutputProfileRef",
        Some(_) if reference(doc).is_none() => "/DestOutputProfileRef is not a dictionary",
        Some(_) => return Vec::new(),
    };
    vec![found(X4P_OUTPUT_INTENT_DEST_PROFILE_REF, message)]
}

/// A.2: the PDF/X intent embeds no profile.
pub(super) fn no_dest_profile(doc: &Document, _: &Settings) -> Vec<Finding> {
    let embedded = intent::the_intent(doc).is_some_and(|i| i.get(b"DestOutputProfile").is_some());
    let message = "the PDF/X output intent has /DestOutputProfile";
    embedded
        .then(|| found(X4P_OUTPUT_INTENT_NO_DEST_PROFILE, message))
        .into_iter()
        .collect()
}

/// The finding `message` from `check` when the reference exists and `wrong` holds for it.
fn rule(doc: &Document, check: &'static Check, wrong: &dyn Fn(&Dict) -> Option<String>) -> Vec<Finding> {
    reference(doc)
        .and_then(wrong)
        .map(|m| found(check, m))
        .into_iter()
        .collect()
}

/// A.2: `/ProfileName` is a text string.
pub(super) fn profile_name(doc: &Document, _: &Settings) -> Vec<Finding> {
    rule(doc, X4P_REF_PROFILE_NAME, &|r| match string(doc, r, b"ProfileName") {
        None => Some("/DestOutputProfileRef has no /ProfileName string".into()),
        Some(name) if text::decode(name).is_none() => Some("/ProfileName is not a valid text string".into()),
        Some(_) => None,
    })
}

/// A.2: `/URLs` is a non-empty array of URL file specifications (`/FS /URL`, with the URL as `/F`).
pub(super) fn urls_present(doc: &Document, _: &Settings) -> Vec<Finding> {
    rule(doc, X4P_REF_URLS, &|r| {
        let items = match entry(doc, r, b"URLs") {
            Some(Value::Array(items)) if !items.is_empty() => items,
            Some(Value::Array(_)) => return Some("/URLs is empty".into()),
            _ => return Some("/DestOutputProfileRef has no /URLs array".into()),
        };
        let url_spec = |item: &Object| {
            doc.resolve(item).and_then(Object::dict).is_some_and(|d| {
                d.get(b"FS").and_then(|f| doc.resolve(f)).and_then(Object::name) == Some(b"URL")
                    && string(doc, d, b"F").is_some()
            })
        };
        let bad = items.iter().filter(|i| !url_spec(i)).count();
        (bad > 0).then(|| {
            format!(
                "{bad} of the {} /URLs entries are not URL file specifications",
                items.len()
            )
        })
    })
}

/// A.2: no `/URLs` file specification has `/EF`, `/DOS`, `/Mac` or `/Unix`.
pub(super) fn urls_no_ef_platform(doc: &Document, _: &Settings) -> Vec<Finding> {
    forbidden_keys(doc, X4P_REF_URLS_NO_EF_PLATFORM, &FORBIDDEN, "A.2")
}

/// ISO 15930-8 7.2: under PDF/X-5n a `/URLs` file specification may embed the profile with `/EF`; `/DOS`, `/Mac` and
/// `/Unix` stay forbidden.
pub(super) fn x5n_urls_ef_allowed(doc: &Document, _: &Settings) -> Vec<Finding> {
    forbidden_keys(doc, X5_N_URLS_EF_ALLOWED, &[b"DOS", b"Mac", b"Unix"], "7.2")
}

/// A finding from `check` for each of `keys` a `/URLs` file specification has.
fn forbidden_keys(doc: &Document, check: &'static Check, keys: &[&[u8]], clause: &str) -> Vec<Finding> {
    let Some(reference) = reference(doc) else {
        return Vec::new();
    };
    let items = match entry(doc, reference, b"URLs") {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    let specs = items.iter().filter_map(|i| doc.resolve(i)?.dict());
    specs
        .flat_map(|spec| keys.iter().filter(|k| spec.get(k).is_some()))
        .map(|key| {
            let key = String::from_utf8_lossy(key);
            Finding::new(check, format!("a /URLs file specification has /{key}")).citing(clause)
        })
        .collect()
}

/// A.2: `/CheckSum` is a 16-byte string.
pub(super) fn checksum(doc: &Document, _: &Settings) -> Vec<Finding> {
    rule(doc, X4P_REF_CHECKSUM, &|r| match string(doc, r, b"CheckSum") {
        None => Some("/DestOutputProfileRef has no /CheckSum string".into()),
        Some(sum) if sum.len() != 16 => Some(format!("/CheckSum is {} bytes, not 16", sum.len())),
        Some(_) => None,
    })
}

/// A.2: `/ProfileCS` is `GRAY`, `CMYK` or `RGB ` (with its space).
pub(super) fn profile_cs(doc: &Document, _: &Settings) -> Vec<Finding> {
    rule(doc, X4P_REF_PROFILE_CS, &|r| match string(doc, r, b"ProfileCS") {
        None => Some("/DestOutputProfileRef has no /ProfileCS string".into()),
        Some(b"GRAY" | b"CMYK" | b"RGB ") => None,
        Some(other) => Some(format!(
            "/ProfileCS is {:?}, not GRAY, CMYK or \"RGB \"",
            String::from_utf8_lossy(other)
        )),
    })
}

/// A.2: `/ICCVersion` is the profile header's four version bytes, as a string.
pub(super) fn icc_version(doc: &Document, _: &Settings) -> Vec<Finding> {
    rule(doc, X4P_REF_ICC_VERSION, &|r| match string(doc, r, b"ICCVersion") {
        None => Some("/DestOutputProfileRef has no /ICCVersion string".into()),
        Some(version) if version.len() != 4 => Some(format!("/ICCVersion is {} bytes, not 4", version.len())),
        Some(_) => None,
    })
}

/// A.2: the reference has no `/ColorantTable` (that is PDF/X-5n's).
pub(super) fn no_colorant_table(doc: &Document, _: &Settings) -> Vec<Finding> {
    rule(doc, X4P_REF_NO_COLORANT_TABLE, &|r| {
        r.get(b"ColorantTable")
            .map(|_| "/DestOutputProfileRef has /ColorantTable".into())
    })
}

/// A.1, A.2: the file set holds the profile the `/CheckSum` names. Fails when no file on this computer is it and no
/// network URL is left that might be; not evaluated when the file is verified alone or only a network URL remains.
pub(super) fn checksum_matches(doc: &Document, settings: &Settings) -> Result<Vec<Finding>, String> {
    if reference(doc).is_none() {
        return Ok(Vec::new());
    }
    match external(doc, settings) {
        External::Found { .. } => Ok(Vec::new()),
        External::Unresolved { why, network: false } => {
            let message = format!("the file set has no external profile: {why}");
            Ok(vec![Finding::new(X4P_FILESET_CHECKSUM_MATCHES, message).citing("A.2")])
        }
        unresolved => Err(resolved(unresolved).err().unwrap_or_default()),
    }
}

/// A.2: `/ProfileName`, `/ProfileCS` and `/ICCVersion` are the external profile's description (any of its records),
/// colour space signature and version bytes.
pub(super) fn fields_match(doc: &Document, settings: &Settings) -> Result<Vec<Finding>, String> {
    let Some(reference) = reference(doc) else {
        return Ok(Vec::new());
    };
    let (path, data) = profile(doc, settings)?;
    let shown = path.display();
    let mut out = Vec::new();
    let name = string(doc, reference, b"ProfileName").and_then(text::decode);
    if let Some(name) = name.filter(|n| !icc::descriptions(&data).contains(n)) {
        out.push(format!("/ProfileName {name:?} is not the description of {shown}"));
    }
    for (key, range) in [(b"ProfileCS".as_slice(), 16..20), (b"ICCVersion", 8..12)] {
        let declared = string(doc, reference, key);
        if declared.is_some_and(|d| Some(d) != data.get(range)) {
            out.push(format!(
                "/{} is not the header of {shown}",
                String::from_utf8_lossy(key)
            ));
        }
    }
    Ok(out
        .into_iter()
        .map(|m| Finding::new(X4P_FILESET_FIELDS_MATCH, m).citing("A.2"))
        .collect())
}

/// A.1 applies 6.4.2.1 to the external profile: an output profile (`prtr`), ICC version 2 or 4, for gray, RGB or
/// CMYK (6.4.1).
pub(super) fn profile_valid(doc: &Document, settings: &Settings) -> Result<Vec<Finding>, String> {
    if reference(doc).is_none() {
        return Ok(Vec::new());
    }
    let (path, data) = profile(doc, settings)?;
    let shown = path.display();
    let Some(header) = icc::header(&data) else {
        let message = format!("{shown} is not an ICC profile");
        return Ok(vec![Finding::new(X4P_FILESET_PROFILE_VALID, message).citing("A.1")]);
    };
    let signature = |b: [u8; 4]| String::from_utf8_lossy(&b).trim_end().to_owned();
    let (major, minor) = header.version;
    let problems = [
        (header.class != *b"prtr").then(|| format!("{shown} has class {}, not prtr", signature(header.class))),
        (!matches!(major, 2 | 4)).then(|| format!("{shown} is ICC version {major}.{minor}, not 2 or 4")),
        (!matches!(&header.space, b"GRAY" | b"RGB " | b"CMYK"))
            .then(|| format!("{shown} is for {}, not gray, RGB or CMYK", signature(header.space))),
    ];
    Ok(problems
        .into_iter()
        .flatten()
        .map(|m| Finding::new(X4P_FILESET_PROFILE_VALID, m).citing("A.1"))
        .collect())
}

/// For 6.4.3.3 under PDF/X-4p: whether a profile is the intent's, judged without the external profile when it is not
/// found. `Some(true)` when `data`'s MD5 is the `/CheckSum`, `Some(false)` when the intent is not CMYK, so no
/// four-colour profile can be its; `None` when that cannot be told.
pub(super) fn is_intent_without_profile(doc: &Document, data: &[u8]) -> Option<bool> {
    let reference = reference(doc)?;
    if string(doc, reference, b"CheckSum").is_some_and(|c| c == md5::compute(data).0) {
        return Some(true);
    }
    (string(doc, reference, b"ProfileCS") != Some(b"CMYK")).then_some(false)
}

/// The reference's `/ColorantTable`, when it is an array of names (ISO 15930-8 7.2).
pub(super) fn colorant_table(doc: &Document) -> Option<Vec<&[u8]>> {
    match entry(doc, reference(doc)?, b"ColorantTable")? {
        Value::Array(items) => items.iter().map(|i| doc.resolve(i)?.name()).collect(),
        _ => None,
    }
}

/// The number of colorants an xCLR signature (`2CLR` to `FCLR`) names.
fn xclr_channels(signature: &[u8]) -> Option<usize> {
    match signature {
        [digit, b'C', b'L', b'R'] => char::from(*digit)
            .to_digit(16)
            .filter(|n| *n >= 2)
            .and_then(|n| usize::try_from(n).ok()),
        _ => None,
    }
}

/// ISO 15930-8 7.2: `/ProfileCS` is an xCLR colour space signature.
pub(super) fn x5n_profile_cs_xclr(doc: &Document, _: &Settings) -> Vec<Finding> {
    let wrong = |r: &Dict| match string(doc, r, b"ProfileCS") {
        None => Some("/DestOutputProfileRef has no /ProfileCS string".to_owned()),
        Some(cs) if xclr_channels(cs).is_some() => None,
        Some(cs) => Some(format!(
            "/ProfileCS is {:?}, not an xCLR signature",
            String::from_utf8_lossy(cs)
        )),
    };
    reference(doc)
        .and_then(wrong)
        .map(|m| Finding::new(X5_N_PROFILECS_XCLR, m).citing("7.2"))
        .into_iter()
        .collect()
}

/// ISO 15930-8 7.2: the reference has `/ColorantTable`, an array of names.
pub(super) fn x5n_colorant_table_present(doc: &Document, _: &Settings) -> Vec<Finding> {
    let wrong = |r: &Dict| match r.get(b"ColorantTable") {
        None => Some("/DestOutputProfileRef has no /ColorantTable".to_owned()),
        Some(_) if colorant_table(doc).is_none() => Some("/ColorantTable is not an array of names".to_owned()),
        Some(_) => None,
    };
    reference(doc)
        .and_then(wrong)
        .map(|m| Finding::new(X5_N_COLORANT_TABLE_PRESENT, m).citing("7.2"))
        .into_iter()
        .collect()
}

/// ISO 15930-8 7.2 with 3.13 (X5-Q4): `/ColorantTable` names as many colorants as `/ProfileCS` has channels. A warning:
/// the rule is derived, not stated.
pub(super) fn x5n_colorant_table_count(doc: &Document, _: &Settings) -> Vec<Finding> {
    let channels = reference(doc)
        .and_then(|r| string(doc, r, b"ProfileCS"))
        .and_then(xclr_channels);
    let (Some(channels), Some(table)) = (channels, colorant_table(doc)) else {
        return Vec::new();
    };
    let message = format!(
        "/ColorantTable names {} colorants, but /ProfileCS has {channels}",
        table.len()
    );
    (table.len() != channels)
        .then(|| Finding::new(X5_N_COLORANT_TABLE_COUNT, message).citing("7.2"))
        .into_iter()
        .collect()
}

/// ISO 15930-8 7.2 (X5-Q4): `3CLR` does not stand for RGB, nor `4CLR` for CMYK, told by `/ColorantTable` naming exactly
/// those inks. A warning: the names are the only sign.
pub(super) fn x5n_3clr_4clr(doc: &Document, _: &Settings) -> Vec<Finding> {
    let cs = reference(doc).and_then(|r| string(doc, r, b"ProfileCS"));
    let Some(mut table) = colorant_table(doc) else {
        return Vec::new();
    };
    table.sort_unstable();
    let process = match cs {
        Some(b"3CLR") if table == [b"Blue".as_slice(), b"Green", b"Red"] => "RGB",
        Some(b"4CLR") if table == [b"Black".as_slice(), b"Cyan", b"Magenta", b"Yellow"] => "CMYK",
        _ => return Vec::new(),
    };
    let message = format!("/ProfileCS is an xCLR space whose /ColorantTable is the {process} process inks");
    vec![Finding::new(X5_N_3CLR_4CLR_NOT_RGB_CMYK, message).citing("7.2")]
}

/// ISO 15930-8 7.1: the external profile is an ISO 15076-1 (ICC version 4.2 or later) output profile. Without it, the
/// reference's `/ICCVersion` can still show an earlier version.
pub(super) fn x5n_profile_iso15076(doc: &Document, settings: &Settings) -> Result<Vec<Finding>, String> {
    let Some(reference) = reference(doc) else {
        return Ok(Vec::new());
    };
    let before_15076 = |major: u8, minor: u8| major < 4 || (major == 4 && minor < 0x20);
    let finding = |m: String| Finding::new(X5_N_PROFILE_ISO15076, m).citing("7.1");
    match profile(doc, settings) {
        Ok((path, data)) => {
            let shown = path.display();
            let Some(header) = icc::header(&data) else {
                return Ok(vec![finding(format!("{shown} is not an ICC profile"))]);
            };
            let [major, minor] = data
                .get(8..10)
                .and_then(|v| v.first_chunk::<2>())
                .copied()
                .unwrap_or_default();
            let mut out = Vec::new();
            if before_15076(major, minor) {
                out.push(finding(format!(
                    "{shown} is ICC version {}.{}, earlier than ISO 15076-1",
                    header.version.0, header.version.1
                )));
            }
            if header.class != *b"prtr" {
                out.push(finding(format!(
                    "{shown} has class {}, not prtr",
                    String::from_utf8_lossy(&header.class)
                )));
            }
            Ok(out)
        }
        Err(why) => match string(doc, reference, b"ICCVersion").and_then(<[u8]>::first_chunk::<2>) {
            Some([major, minor]) if before_15076(*major, *minor) => Ok(vec![finding(format!(
                "/ICCVersion is {major}.{}, earlier than ISO 15076-1",
                minor >> 4
            ))]),
            _ => Err(why),
        },
    }
}

/// ISO 15930-8 7.2: `/ColorantTable` names the colorants of the external profile's `colorantTableTag`, in its order.
pub(super) fn x5n_colorant_table_matches_profile(doc: &Document, settings: &Settings) -> Result<Vec<Finding>, String> {
    let Some(table) = colorant_table(doc) else {
        return Ok(Vec::new());
    };
    let (path, data) = profile(doc, settings)?;
    let message = match icc::colorant_names(&data) {
        Some(names) if names.iter().map(String::as_bytes).eq(table.iter().copied()) => return Ok(Vec::new()),
        Some(names) => format!(
            "/ColorantTable is not the colorants of {}, which are {}",
            path.display(),
            names.join(", ")
        ),
        None => format!("{} has no colorantTableTag", path.display()),
    };
    Ok(vec![
        Finding::new(X5_N_COLORANT_TABLE_MATCHES_PROFILE, message).citing("7.2"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    /// A minimal ICC profile: class, colour space and version, and a v2 `desc` tag holding `name`.
    fn icc(class: [u8; 4], space: [u8; 4], major: u8, name: &str) -> Vec<u8> {
        let mut tag = b"desc\0\0\0\0".to_vec();
        tag.extend(u32::try_from(name.len() + 1).unwrap().to_be_bytes());
        tag.extend(name.as_bytes());
        tag.push(0);
        let mut out = vec![0u8; 128];
        out[8] = major;
        out[12..16].copy_from_slice(&class);
        out[16..20].copy_from_slice(&space);
        out[36..40].copy_from_slice(b"acsp");
        out.extend(1u32.to_be_bytes());
        out.extend(b"desc");
        out.extend(144u32.to_be_bytes());
        out.extend(u32::try_from(tag.len()).unwrap().to_be_bytes());
        out.extend(tag);
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().concat()
    }

    /// A one-page file whose PDF/X intent has `entries`.
    fn doc(entries: &str) -> Document {
        let catalog =
            format!("<< /Pages 2 0 R /OutputIntents [<< /S /GTS_PDFX /OutputConditionIdentifier (x) {entries} >>] >>");
        let objects: [(u32, &[u8]); 3] = [
            (1, catalog.as_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>"),
        ];
        Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }

    /// A reference to `profile` at `urls`, declaring its own name, space and version.
    fn reference(profile: &[u8], urls: &str) -> String {
        format!(
            "/DestOutputProfileRef << /ProfileName (FOGRA39) /URLs [{urls}] /CheckSum <{}> /ProfileCS (CMYK) \
             /ICCVersion <{}> >>",
            hex(&md5::compute(profile).0),
            hex(profile.get(8..12).unwrap_or_default()),
        )
    }

    fn set(folder: &std::path::Path) -> Settings {
        Settings {
            files: Files::Set(folder.to_owned()),
            ..Level::X4p.into()
        }
    }

    fn messages(found: Vec<Finding>) -> Vec<String> {
        found.into_iter().map(|f| f.message).collect()
    }

    #[test]
    fn the_reference_has_its_keys() {
        let runs: [fn(&Document, &Settings) -> Vec<Finding>; 9] = [
            dest_profile_ref,
            no_dest_profile,
            profile_name,
            urls_present,
            urls_no_ef_platform,
            checksum,
            profile_cs,
            icc_version,
            no_colorant_table,
        ];
        let found = |entries: &str| -> Vec<String> {
            let doc = doc(entries);
            runs.iter()
                .flat_map(|run| messages(run(&doc, &Level::X4p.into())))
                .collect()
        };
        let good = reference(&icc(*b"prtr", *b"CMYK", 2, "FOGRA39"), "<< /FS /URL /F (p.icc) >>");
        assert_eq!(found(&good), Vec::<String>::new());
        assert_eq!(
            found("/DestOutputProfile 9 0 R"),
            [
                "the PDF/X output intent has no /DestOutputProfileRef",
                "the PDF/X output intent has /DestOutputProfile"
            ]
        );
        assert_eq!(
            found("/DestOutputProfileRef 5"),
            ["/DestOutputProfileRef is not a dictionary"]
        );
        assert_eq!(
            found("/DestOutputProfileRef << /ColorantTable [] /URLs [] >>"),
            [
                "/DestOutputProfileRef has no /ProfileName string",
                "/URLs is empty",
                "/DestOutputProfileRef has no /CheckSum string",
                "/DestOutputProfileRef has no /ProfileCS string",
                "/DestOutputProfileRef has no /ICCVersion string",
                "/DestOutputProfileRef has /ColorantTable",
            ]
        );
        assert_eq!(
            found(
                "/DestOutputProfileRef << /ProfileName <FEFF00> /URLs [(p.icc) << /FS /URL /F (q) /EF << >> /Mac (m) >>] \
                 /CheckSum (short) /ProfileCS (Lab ) /ICCVersion (2) >>"
            ),
            [
                "/ProfileName is not a valid text string",
                "1 of the 2 /URLs entries are not URL file specifications",
                "a /URLs file specification has /EF",
                "a /URLs file specification has /Mac",
                "/CheckSum is 5 bytes, not 16",
                "/ProfileCS is \"Lab \", not GRAY, CMYK or \"RGB \"",
                "/ICCVersion is 1 bytes, not 4",
            ]
        );
        assert_eq!(
            found("/DestOutputProfileRef << /URLs 3 >>")[1],
            "/DestOutputProfileRef has no /URLs array"
        );
        assert_eq!(found(""), ["the PDF/X output intent has no /DestOutputProfileRef"]);
    }

    #[test]
    fn a_file_set_holds_the_profile_the_checksum_names() {
        let folder = std::env::temp_dir().join(format!("proof-pdf-x4p-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let profile = icc(*b"prtr", *b"CMYK", 2, "FOGRA39");
        std::fs::write(folder.join("p.icc"), &profile).unwrap();
        std::fs::write(folder.join("other.icc"), icc(*b"mntr", *b"Lab ", 5, "x")).unwrap();
        let judge = |entries: &str, settings: &Settings| {
            let doc = doc(entries);
            [checksum_matches, fields_match, profile_valid].map(|j| j(&doc, settings).map(messages))
        };
        let ok = || Ok(Vec::new());
        let here = set(&folder);

        // Found after a candidate that is not it; the reference's fields are the profile's.
        let found = reference(&profile, "(other.icc) << /FS /URL /F (p.icc) >>");
        assert_eq!(judge(&found, &here), [ok(), ok(), ok()]);
        assert_eq!(space(&doc(&found), &here), Some(*b"CMYK"));

        // The fields differ from the profile's header and description.
        let wrong = found.replace("(FOGRA39)", "(FOGRA51)").replace("(CMYK)", "(RGB )");
        let [_, fields, _] = judge(&wrong, &here);
        let shown = folder.join("p.icc").display().to_string();
        assert_eq!(
            fields,
            Ok(vec![
                format!("/ProfileName \"FOGRA51\" is not the description of {shown}"),
                format!("/ProfileCS is not the header of {shown}"),
            ])
        );

        // An invalid profile at the right checksum: the wrong class, version and space.
        let bad = icc(*b"mntr", *b"Lab ", 5, "x");
        let [_, _, valid] = judge(&reference(&bad, "(other.icc)"), &here);
        let shown = folder.join("other.icc").display().to_string();
        assert_eq!(
            valid,
            Ok(vec![
                format!("{shown} has class mntr, not prtr"),
                format!("{shown} is ICC version 5.0, not 2 or 4"),
                format!("{shown} is for Lab, not gray, RGB or CMYK"),
            ])
        );
        std::fs::write(folder.join("short.icc"), b"tiny").unwrap();
        let [_, _, valid] = judge(&reference(b"tiny", "(short.icc)"), &here);
        assert_eq!(
            valid,
            Ok(vec![format!(
                "{} is not an ICC profile",
                folder.join("short.icc").display()
            )])
        );

        // Missing, or not the profile: fails, unless a network URL might still hold it.
        let lost = reference(&profile, "(gone.icc) (other.icc)");
        let [sum, fields, _] = judge(&lost, &here);
        let md5 = hex(&md5::compute(icc(*b"mntr", *b"Lab ", 5, "x")).0).to_lowercase();
        let why = format!("no file at gone.icc; other.icc has MD5 {md5}, not the /CheckSum");
        assert_eq!(sum, Ok(vec![format!("the file set has no external profile: {why}")]));
        assert_eq!(fields, Err(format!("no external profile: {why}")));
        let [sum, ..] = judge(&reference(&profile, ""), &here);
        assert_eq!(
            sum,
            Ok(vec![
                "the file set has no external profile: /URLs names no profile".to_owned()
            ])
        );
        let [sum, ..] = judge(&reference(&profile, "(ftp://example.com/p.icc)"), &here);
        assert_eq!(sum, Err("every URL needs a network, which is not used".into()));
        let remote = reference(&profile, "(gone.icc) (https://example.com/p.icc)");
        let [sum, ..] = judge(&remote, &here);
        assert_eq!(
            sum,
            Err("no file at gone.icc; the other URLs need a network, which is not used".into())
        );
        assert_eq!(space(&doc(&remote), &here), Some(*b"CMYK"));

        // Alone, or with nothing to confirm a candidate by: not evaluated.
        let alone = Settings::from(Level::X4p);
        let [sum, ..] = judge(&found, &alone);
        assert_eq!(
            sum,
            Err("the file is verified alone, without its external profile".into())
        );
        let [sum, ..] = judge("/DestOutputProfileRef << /URLs [(p.icc)] >>", &here);
        assert_eq!(
            sum,
            Err("without a 16-byte /CheckSum no profile can be confirmed".into())
        );

        // No reference: other rows say why, these find nothing; nor is there a space to take.
        assert_eq!(judge("", &here), [ok(), ok(), ok()]);
        assert_eq!(space(&doc(""), &here), None);
        assert_eq!(space(&doc(&found), &Level::X4.into()), None);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn odd_references_find_nothing_or_say_why() {
        let bare = {
            let objects: [(u32, &[u8]); 3] = [
                (1, b"<< /Pages 2 0 R >>"),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
                (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>"),
            ];
            Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
        };
        let x4p = Level::X4p.into();
        assert!(dest_profile_ref(&bare, &x4p).is_empty() && urls_no_ef_platform(&bare, &x4p).is_empty());
        let dangling = doc("/DestOutputProfileRef 99 0 R");
        assert_eq!(
            messages(dest_profile_ref(&dangling, &x4p)),
            ["/DestOutputProfileRef is not a dictionary"]
        );
        let numbers = doc("/DestOutputProfileRef << /ProfileName 5 /URLs [5 99 0 R] /ProfileCS 1 >>");
        assert_eq!(
            messages(profile_name(&numbers, &x4p)),
            ["/DestOutputProfileRef has no /ProfileName string"]
        );
        assert_eq!(space(&numbers, &x4p), None);
        assert_eq!(urls_no_ef_platform(&numbers, &x4p), []);
        let profile = icc(*b"prtr", *b"CMYK", 2, "FOGRA39");
        let here = set(std::path::Path::new("/nowhere"));
        let items = reference(&profile, "5 99 0 R");
        assert_eq!(
            checksum_matches(&doc(&items), &here).map(messages),
            Ok(vec![
                "the file set has no external profile: /URLs names no profile".to_owned()
            ])
        );
        let no_array = reference(&profile, "").replace("/URLs []", "/URLs 1");
        assert_eq!(
            checksum_matches(&doc(&no_array), &here).map(messages),
            Ok(vec![
                "the file set has no external profile: /URLs names no profile".to_owned()
            ])
        );
    }

    #[test]
    fn without_the_profile_the_checksum_or_profile_cs_can_still_tell() {
        let profile = icc(*b"prtr", *b"CMYK", 2, "FOGRA39");
        let cmyk = doc(&reference(&profile, "(p.icc)"));
        assert_eq!(is_intent_without_profile(&cmyk, &profile), Some(true));
        assert_eq!(is_intent_without_profile(&cmyk, b"other"), None);
        let rgb = doc(&reference(&profile, "(p.icc)").replace("(CMYK)", "(RGB )"));
        assert_eq!(is_intent_without_profile(&rgb, b"other"), Some(false));
        assert_eq!(is_intent_without_profile(&doc(""), b"other"), None);
    }

    /// An n-colorant profile: `space`, version `major`.`minor`, class `class` and a `clrt` tag naming `inks`.
    fn xclr(space: [u8; 4], major: u8, class: [u8; 4], inks: &[&str]) -> Vec<u8> {
        let mut clrt = b"clrt\0\0\0\0".to_vec();
        clrt.extend(u32::try_from(inks.len()).unwrap().to_be_bytes());
        for ink in inks {
            let mut entry = ink.as_bytes().to_vec();
            entry.resize(38, 0);
            clrt.extend(entry);
        }
        let mut out = vec![0u8; 128];
        out[8] = major;
        out[9] = 0x30;
        out[12..16].copy_from_slice(&class);
        out[16..20].copy_from_slice(&space);
        out[36..40].copy_from_slice(b"acsp");
        out.extend(1u32.to_be_bytes());
        out.extend(b"clrt");
        out.extend(144u32.to_be_bytes());
        out.extend(u32::try_from(clrt.len()).unwrap().to_be_bytes());
        out.extend(clrt);
        out
    }

    /// A file whose PDF/X intent has `entries`, with `more` objects.
    fn doc_with(entries: &str, more: &[(u32, Vec<u8>)]) -> Document {
        let catalog = format!("<< /Pages 2 0 R /OutputIntents [<< /S /GTS_PDFX {entries} >>] >>");
        let mut objects: Vec<(u32, &[u8])> = vec![
            (1, catalog.as_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>"),
        ];
        objects.extend(more.iter().map(|(n, b)| (*n, b.as_slice())));
        Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }

    #[test]
    fn pdf_x_5n_reference_rows() {
        let x5n = Level::X5n.into();
        let found = |entries: &str, run: fn(&Document, &Settings) -> Vec<Finding>| -> Vec<String> {
            messages(run(
                &doc_with(&format!("/DestOutputProfileRef << {entries} >>"), &[]),
                &x5n,
            ))
        };
        assert_eq!(
            found("", x5n_profile_cs_xclr),
            ["/DestOutputProfileRef has no /ProfileCS string"]
        );
        for bad in ["CMYK", "1CLR", "GCLR"] {
            let message = format!("/ProfileCS is {bad:?}, not an xCLR signature");
            assert_eq!(found(&format!("/ProfileCS ({bad})"), x5n_profile_cs_xclr), [message]);
        }
        assert_eq!(found("/ProfileCS (FCLR)", x5n_profile_cs_xclr), Vec::<String>::new());
        assert_eq!(
            found("", x5n_colorant_table_present),
            ["/DestOutputProfileRef has no /ColorantTable"]
        );
        let not_names = "/ColorantTable [/Cyan 5]";
        assert_eq!(
            found(not_names, x5n_colorant_table_present),
            ["/ColorantTable is not an array of names"]
        );
        assert_eq!(
            found("/ColorantTable 5", x5n_colorant_table_present),
            ["/ColorantTable is not an array of names"]
        );
        let dangling = "/ColorantTable [/Cyan 99 0 R]";
        assert_eq!(
            found(dangling, x5n_colorant_table_present),
            ["/ColorantTable is not an array of names"]
        );
        let four = "/ProfileCS (5CLR) /ColorantTable [/Cyan /Magenta /Yellow /Black]";
        assert_eq!(
            found(four, x5n_colorant_table_count),
            ["/ColorantTable names 4 colorants, but /ProfileCS has 5"]
        );
        assert_eq!(found(four, x5n_colorant_table_present), Vec::<String>::new());
        assert_eq!(
            found(
                "/ProfileCS (4CLR) /ColorantTable [/A /B /C /D]",
                x5n_colorant_table_count
            ),
            Vec::<String>::new()
        );
        let rgb = "/ProfileCS (3CLR) /ColorantTable [/Red /Green /Blue]";
        assert_eq!(
            found(rgb, x5n_3clr_4clr),
            ["/ProfileCS is an xCLR space whose /ColorantTable is the RGB process inks"]
        );
        let cmyk = "/ProfileCS (4CLR) /ColorantTable [/Yellow /Black /Cyan /Magenta]";
        assert_eq!(
            found(cmyk, x5n_3clr_4clr),
            ["/ProfileCS is an xCLR space whose /ColorantTable is the CMYK process inks"]
        );
        assert_eq!(found(four, x5n_3clr_4clr), Vec::<String>::new());
        assert_eq!(found("", x5n_3clr_4clr), Vec::<String>::new());
    }

    #[test]
    fn pdf_x_5n_profiles_may_be_embedded() {
        let good = xclr(*b"2CLR", 4, *b"prtr", &["Orange", "Green"]);
        let here = Settings {
            files: Files::Set(std::env::temp_dir()),
            ..Level::X5n.into()
        };
        let sum = |d: &[u8]| hex(&md5::compute(d).0);
        // A URL file specification may embed the profile (7.2); one with no URL or an /EF that is not a stream names none.
        let embedded = |ef: &str| {
            doc_with(
                &format!(
                    "/DestOutputProfileRef << /URLs [<< /FS /URL /F (e.icc) /EF << /F {ef} >> >> << /FS /URL >>] /CheckSum <{}> >>",
                    sum(&good)
                ),
                &[(9, {
                    let mut s = format!("<< /Length {} >>\nstream\n", good.len()).into_bytes();
                    s.extend(&good);
                    s.extend(b"\nendstream");
                    s
                })],
            )
        };
        assert!(matches!(external(&embedded("9 0 R"), &here), External::Found { .. }));
        assert!(matches!(
            external(&embedded("<< >>"), &here),
            External::Unresolved { .. }
        ));
        let no_file = format!(
            "/DestOutputProfileRef << /URLs [<< /FS /URL /F (e.icc) /EF << >> >>] /CheckSum <{}> >>",
            sum(&good)
        );
        assert!(matches!(
            external(&doc_with(&no_file, &[]), &here),
            External::Unresolved { .. }
        ));
    }

    #[test]
    fn pdf_x_5n_profile_rows() {
        let folder = std::env::temp_dir().join(format!("proof-pdf-x5n-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let good = xclr(*b"2CLR", 4, *b"prtr", &["Orange", "Green"]);
        let old = xclr(*b"2CLR", 2, *b"mntr", &["Orange", "Green"]);
        let mut bare = good.clone();
        bare[128..132].copy_from_slice(&0u32.to_be_bytes());
        for (name, data) in [
            ("good.icc", &good),
            ("old.icc", &old),
            ("bare.icc", &bare),
            ("text.icc", &b"text".to_vec()),
        ] {
            std::fs::write(folder.join(name), data).unwrap();
        }
        let here = Settings {
            files: Files::Set(folder.clone()),
            ..Level::X5n.into()
        };
        let sum = |d: &[u8]| hex(&md5::compute(d).0);
        let refer = |file: &str, data: &[u8], extra: &str| {
            doc_with(
                &format!(
                    "/DestOutputProfileRef << /URLs [({file})] /CheckSum <{}> /ColorantTable [/Orange /Green] {extra} >>",
                    sum(data)
                ),
                &[],
            )
        };
        let shown = |f: &str| folder.join(f).display().to_string();
        let both = |d: &Document| {
            (
                x5n_profile_iso15076(d, &here).map(messages),
                x5n_colorant_table_matches_profile(d, &here).map(messages),
            )
        };
        assert_eq!(both(&refer("good.icc", &good, "")), (Ok(vec![]), Ok(vec![])));
        let (iso, _) = both(&refer("old.icc", &old, ""));
        assert_eq!(
            iso,
            Ok(vec![
                format!("{} is ICC version 2.3, earlier than ISO 15076-1", shown("old.icc")),
                format!("{} has class mntr, not prtr", shown("old.icc")),
            ])
        );
        let (_, matches) = both(&refer("bare.icc", &bare, ""));
        assert_eq!(
            matches,
            Ok(vec![format!("{} has no colorantTableTag", shown("bare.icc"))])
        );
        let (iso, _) = both(&refer("text.icc", b"text", ""));
        assert_eq!(iso, Ok(vec![format!("{} is not an ICC profile", shown("text.icc"))]));
        let swapped = doc_with(
            &format!(
                "/DestOutputProfileRef << /URLs [(good.icc)] /CheckSum <{}> /ColorantTable [/Green /Orange] >>",
                sum(&good)
            ),
            &[],
        );
        let (_, matches) = both(&swapped);
        let want = format!(
            "/ColorantTable is not the colorants of {}, which are Orange, Green",
            shown("good.icc")
        );
        assert_eq!(matches, Ok(vec![want]));
        // Without the profile, /ICCVersion can still show an earlier version.
        let alone = Settings::from(Level::X5n);
        let v2 = refer("good.icc", &good, "/ICCVersion <02100000>");
        assert_eq!(
            x5n_profile_iso15076(&v2, &alone).map(messages),
            Ok(vec!["/ICCVersion is 2.1, earlier than ISO 15076-1".to_owned()])
        );
        let v4 = refer("good.icc", &good, "/ICCVersion <04300000>");
        assert!(x5n_profile_iso15076(&v4, &alone).is_err());
        assert_eq!(x5n_profile_iso15076(&doc_with("", &[]), &alone), Ok(vec![]));
        assert_eq!(
            x5n_colorant_table_matches_profile(&doc_with("", &[]), &alone),
            Ok(vec![])
        );
        assert!(x5n_colorant_table_matches_profile(&v2, &alone).is_err());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
