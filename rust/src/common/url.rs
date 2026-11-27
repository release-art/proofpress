//! URLs in file specifications (ISO 32000-2 7.11.5, RFC 3986) as places on this computer. Nothing is fetched: a URL
//! with a scheme other than `file` is only reachable over a network.

use std::path::{Path, PathBuf};

/// Where a URL points.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// A file on this computer: a relative reference resolved against the folder given, or a `file:` URL.
    Local(PathBuf),
    /// Somewhere only a network reaches (`http:`, `ftp:` and every other scheme).
    Network,
}

/// Where `url` points, relative references resolved against `folder`. The query and fragment are dropped and
/// percent-escapes decoded; a `file:` URL's host must be empty or `localhost`.
pub(crate) fn place(url: &str, folder: &Path) -> Place {
    let url = url.split(['?', '#']).next().unwrap_or_default();
    match scheme(url) {
        None => Place::Local(folder.join(decode(url))),
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            let rest = url.get(scheme.len() + 1..).unwrap_or_default();
            let path = match rest.strip_prefix("//") {
                Some(authority) => {
                    let (host, path) = authority.split_at(authority.find('/').unwrap_or(authority.len()));
                    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                        return Place::Network;
                    }
                    path
                }
                None => rest,
            };
            Place::Local(folder.join(decode(path)))
        }
        Some(_) => Place::Network,
    }
}

/// The scheme (RFC 3986 3.1): a letter, then letters, digits, `+`, `-` or `.`, before the first `:`. A single letter
/// is read as a Windows drive, not a scheme.
fn scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (valid && scheme.len() > 1).then_some(scheme)
}

/// `text` with every `%XX` escape decoded; an escape that is not two hex digits stays as written.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        if let (b'%', Some(value)) = (b, hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            out.push(value);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_local_files_or_network_places() {
        let here = Path::new("/jobs");
        let local = |p: &str| Place::Local(PathBuf::from(p));
        assert_eq!(place("profiles/FOGRA39.icc", here), local("/jobs/profiles/FOGRA39.icc"));
        assert_eq!(place("a%20b.icc?x=1#top", here), local("/jobs/a b.icc"));
        assert_eq!(place("100%.icc", here), local("/jobs/100%.icc"));
        assert_eq!(place("file:///srv/p.icc", here), local("/srv/p.icc"));
        assert_eq!(place("FILE://localhost/srv/p.icc", here), local("/srv/p.icc"));
        assert_eq!(place("file:p.icc", here), local("/jobs/p.icc"));
        assert_eq!(place("file://server/p.icc", here), Place::Network);
        assert_eq!(place("https://www.color.org/p.icc", here), Place::Network);
        // A drive letter is a path, not a scheme; how it joins the folder is the platform's rule.
        assert_eq!(place("C:p.icc", here), Place::Local(here.join("C:p.icc")));
        assert_eq!(place("9x:p.icc", here), local("/jobs/9x:p.icc"));
        assert_eq!(place("svn+ssh://host/p.icc", here), Place::Network);
        assert_eq!(place("my file:p.icc", here), local("/jobs/my file:p.icc"));
    }
}
