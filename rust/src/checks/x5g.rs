//! PDF/X-5g and X-5pg (ISO 15930-8 clauses 8 and 9) as the container alone shows them: reference `XObjects`, the form
//! proxies that place a page of another file (8.2, ISO 32000-1 8.10.4), and the XMP that names that file (8.3).

use super::Settings;
use super::metadata::MM;
use crate::common::xmp::{self, Value as Xmp};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Check, Finding, Location, X5_G_DOCUMENT_ID_UUID, X5_G_PROXY_IS_FORM, X5_G_PROXY_METADATA, X5_G_PROXY_RENDITION_OF,
    X5_G_REF_ID_REQUIRED, X5_G_REF_PRESENT,
};

/// The `stRef` namespace of XMP's `ResourceRef` structure.
pub(super) const ST_REF: &str = "http://ns.adobe.com/xap/1.0/sType/ResourceRef#";

/// The `RenditionOf` fields 8.3 requires, spelled as ISO 15930-8:2010 Technical Corrigendum 1 (2011) corrects them and
/// as the XMP specification spells them (X5-Q6).
pub(super) const RENDITION_FIELDS: [&str; 3] = ["documentID", "versionID", "renditionClass"];

/// A form `XObject` that places another file's page: its object number, its dictionary and its `/Ref`.
pub(super) struct Proxy<'a> {
    pub(super) num: u32,
    pub(super) form: &'a Dict,
    pub(super) reference: &'a Dict,
}

/// Every stream with a `/Ref` dictionary: its object number, dictionary and `/Ref`.
fn referring(doc: &Document) -> Vec<(u32, &Dict, &Dict)> {
    let streams = doc
        .objects()
        .filter_map(|(num, object, stream, _)| stream.and(object.dict()).map(|d| (num, d)));
    streams
        .filter_map(|(num, dict)| {
            let reference = dict.get(b"Ref").and_then(|r| doc.resolve(r)).and_then(Object::dict);
            reference.map(|r| (num, dict, r))
        })
        .collect()
}

fn subtype<'a>(doc: &'a Document, dict: &'a Dict) -> Option<&'a [u8]> {
    dict.get(b"Subtype").and_then(|s| doc.resolve(s)).and_then(Object::name)
}

/// Every form proxy in the file, in object order.
pub(super) fn proxies(doc: &Document) -> Vec<Proxy<'_>> {
    referring(doc)
        .into_iter()
        .filter(|(_, form, _)| subtype(doc, form) == Some(b"Form"))
        .map(|(num, form, reference)| Proxy { num, form, reference })
        .collect()
}

fn at(check: &'static Check, num: u32, message: impl std::fmt::Display, clause: &str) -> Finding {
    Finding::new(check, format!("object {num}: {message}"))
        .citing(clause)
        .at(vec![Location::Object(num, 0)])
}

/// 8.2, Table A.1 (X5-Q1, X5-Q2): the file places something through a reference `XObject`. A warning: that print content
/// was left out of the file cannot be seen, only the proxies that stand for it.
pub(super) fn ref_present(doc: &Document, _: &Settings) -> Vec<Finding> {
    let none = proxies(doc).is_empty();
    let message = "no form XObject has /Ref, so nothing is placed from another file";
    none.then(|| Finding::new(X5_G_REF_PRESENT, message).citing("8.2"))
        .into_iter()
        .collect()
}

/// 8.2: a reference is made through a form `XObject`, the proxy, and nothing else.
pub(super) fn proxy_is_form(doc: &Document, _: &Settings) -> Vec<Finding> {
    referring(doc)
        .into_iter()
        .filter(|(_, dict, _)| subtype(doc, dict) != Some(b"Form"))
        .map(|(num, dict, _)| {
            let what =
                subtype(doc, dict).map_or("a stream".to_owned(), |s| format!("a /{}", String::from_utf8_lossy(s)));
            at(
                X5_G_PROXY_IS_FORM,
                num,
                format!("{what} with /Ref, not a form XObject"),
                "8.2",
            )
        })
        .collect()
}

/// 8.2: every reference has `/ID`, the target's file identifier: an array of two byte strings.
pub(super) fn ref_id_required(doc: &Document, _: &Settings) -> Vec<Finding> {
    proxies(doc)
        .into_iter()
        .filter(|p| file_id(doc, p.reference).is_none())
        .map(|p| at(X5_G_REF_ID_REQUIRED, p.num, "/Ref has no /ID of two strings", "8.2"))
        .collect()
}

/// A `/Ref`'s `/ID`, when it is an array of two strings.
pub(super) fn file_id<'a>(doc: &'a Document, reference: &'a Dict) -> Option<[&'a [u8]; 2]> {
    let items = reference
        .get(b"ID")
        .and_then(|i| doc.resolve(i))
        .and_then(Object::array)?;
    let string = |o: &'a Object| match doc.resolve(o).map(|o| &o.value) {
        Some(Value::String(bytes, _)) => Some(bytes.as_slice()),
        _ => None,
    };
    match items {
        [a, b] => Some([string(a)?, string(b)?]),
        _ => None,
    }
}

/// 8.3: a proxy has a `/Metadata` stream.
pub(super) fn proxy_metadata(doc: &Document, _: &Settings) -> Vec<Finding> {
    proxies(doc)
        .into_iter()
        .filter(|p| metadata(doc, p.form).is_none())
        .map(|p| at(X5_G_PROXY_METADATA, p.num, "the proxy has no /Metadata stream", "8.3"))
        .collect()
}

/// A dictionary's `/Metadata` stream data.
fn metadata(doc: &Document, dict: &Dict) -> Option<Vec<u8>> {
    match dict.get(b"Metadata")?.value {
        Value::Reference(num, _) => doc.stream_data(num),
        _ => None,
    }
}

/// A proxy's `xmpMM:RenditionOf` fields, by name as written, or why there are none.
pub(super) fn rendition_of(doc: &Document, form: &Dict) -> Result<Vec<(String, String)>, &'static str> {
    let data = metadata(doc, form).ok_or("no /Metadata stream")?;
    let packet = xmp::parse(&data).map_err(|_| "XMP that cannot be read")?;
    match packet.get(MM, "RenditionOf") {
        Some(Xmp::Struct(fields)) => Ok(fields
            .iter()
            .filter(|f| f.ns == ST_REF)
            .filter_map(|f| Some((f.name.clone(), f.value.text()?.to_owned())))
            .collect()),
        Some(_) => Err("an xmpMM:RenditionOf that is not a structure"),
        None => Err("no xmpMM:RenditionOf"),
    }
}

/// The field `name` of `fields`, spelled exactly so.
pub(super) fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// 8.3 (X5-Q6): a proxy's XMP has `xmpMM:RenditionOf` with `stRef:documentID`, `versionID` and `renditionClass`,
/// spelled as the XMP specification spells them; a field in another case is named as such.
pub(super) fn proxy_rendition_of(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    // A proxy without metadata is x5.g-proxy-metadata's.
    for proxy in proxies(doc).into_iter().filter(|p| metadata(doc, p.form).is_some()) {
        let fields = match rendition_of(doc, proxy.form) {
            Ok(fields) => fields,
            Err(why) => {
                out.push(at(
                    X5_G_PROXY_RENDITION_OF,
                    proxy.num,
                    format!("the proxy's XMP has {why}"),
                    "8.3",
                ));
                continue;
            }
        };
        for name in RENDITION_FIELDS.into_iter().filter(|n| field(&fields, n).is_none()) {
            let other = fields.iter().find(|(n, _)| n.eq_ignore_ascii_case(name));
            let message = match other {
                Some((spelled, _)) => format!("xmpMM:RenditionOf has stRef:{spelled}, not stRef:{name}"),
                None => format!("xmpMM:RenditionOf has no stRef:{name}"),
            };
            out.push(at(X5_G_PROXY_RENDITION_OF, proxy.num, message, "8.3"));
        }
    }
    out
}

/// 8.3 recommends that a target's document ID be a `uuid:` URI; the proxy's `stRef:documentID` names it.
pub(super) fn document_id_uuid(doc: &Document, _: &Settings) -> Vec<Finding> {
    proxies(doc)
        .into_iter()
        .filter_map(|p| {
            let fields = rendition_of(doc, p.form).ok()?;
            let id = field(&fields, "documentID")?;
            (!id.starts_with("uuid:")).then(|| {
                let message = format!("the target's document ID {id:?} is not a uuid: URI");
                at(X5_G_DOCUMENT_ID_UUID, p.num, message, "8.3")
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    /// XMP with `body` in its description.
    fn xmp(body: &str) -> Vec<u8> {
        format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:xmpMM="{MM}" xmlns:stRef="{ST_REF}">{body}</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
        )
        .into_bytes()
    }

    /// The messages the offline rows give for a page whose `XObjects` are `streams` (object numbers from 20), with object
    /// 9 the XMP `rendition`.
    fn found(streams: &[&str], rendition: &[u8]) -> Vec<String> {
        let names: Vec<String> = (20..20 + streams.len()).map(|n| format!("/X{n} {n} 0 R")).collect();
        let page = format!(
            "<< /Type /Page /Parent 2 0 R /Resources << /XObject << {} >> >> >>",
            names.join(" ")
        );
        let bodies: Vec<Vec<u8>> = streams.iter().map(|d| stream(d, b"")).collect();
        let metadata = stream("/Type /Metadata /Subtype /XML", rendition);
        let mut objects: Vec<(u32, &[u8])> = vec![
            (1, b"<< /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, page.as_bytes()),
            (9, &metadata),
        ];
        objects.extend(bodies.iter().zip(20..).map(|(b, n)| (n, b.as_slice())));
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let runs: [fn(&Document, &Settings) -> Vec<Finding>; 6] = [
            ref_present,
            proxy_is_form,
            ref_id_required,
            proxy_metadata,
            proxy_rendition_of,
            document_id_uuid,
        ];
        runs.iter()
            .flat_map(|r| r(&doc, &Level::X5g.into()))
            .map(|f| f.message)
            .collect()
    }

    const FORM: &str = "/Type /XObject /Subtype /Form /BBox [0 0 1 1]";

    #[test]
    fn odd_references_and_identifiers() {
        let refs = [
            format!("{FORM} /Ref 5"),
            format!("{FORM} /Ref << /ID 3 >> /Metadata << >>"),
            format!("{FORM} /Ref << /ID [<01>] >> /Metadata 9 0 R"),
            format!("{FORM} /Ref << /ID [1 <02>] >> /Metadata 9 0 R"),
            format!("{FORM} /Ref << /ID [<01> 2] >> /Metadata 9 0 R"),
        ];
        let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
        let none = xmp("");
        assert_eq!(
            found(&refs, &none),
            [
                "object 21: /Ref has no /ID of two strings",
                "object 22: /Ref has no /ID of two strings",
                "object 23: /Ref has no /ID of two strings",
                "object 24: /Ref has no /ID of two strings",
                "object 21: the proxy has no /Metadata stream",
                "object 22: the proxy's XMP has no xmpMM:RenditionOf",
                "object 23: the proxy's XMP has no xmpMM:RenditionOf",
                "object 24: the proxy's XMP has no xmpMM:RenditionOf",
            ]
        );
    }

    #[test]
    fn rendition_of_is_a_structure_of_three_fields() {
        let proxy = format!("{FORM} /Ref << /ID [<01> <02>] >> /Metadata 9 0 R");
        let flat = xmp("<xmpMM:RenditionOf>text</xmpMM:RenditionOf>");
        assert_eq!(
            found(&[&proxy], &flat),
            ["object 20: the proxy's XMP has an xmpMM:RenditionOf that is not a structure"]
        );
        assert_eq!(
            found(&[&proxy], b"<not xmp"),
            ["object 20: the proxy's XMP has XMP that cannot be read"]
        );
        let nested = xmp(
            r#"<xmpMM:RenditionOf rdf:parseType="Resource"><stRef:documentID rdf:parseType="Resource"><stRef:x>1</stRef:x></stRef:documentID><stRef:versionID>1</stRef:versionID></xmpMM:RenditionOf>"#,
        );
        let cased = xmp(
            r#"<xmpMM:RenditionOf rdf:parseType="Resource"><stRef:DocumentID>uuid:1</stRef:DocumentID><stRef:versionID>1</stRef:versionID><stRef:renditionClass>default</stRef:renditionClass></xmpMM:RenditionOf>"#,
        );
        assert_eq!(
            found(&[&proxy], &cased),
            ["object 20: xmpMM:RenditionOf has stRef:DocumentID, not stRef:documentID"]
        );
        assert_eq!(
            found(&[&proxy], &nested),
            [
                "object 20: xmpMM:RenditionOf has no stRef:documentID",
                "object 20: xmpMM:RenditionOf has no stRef:renditionClass",
            ]
        );
    }
}
