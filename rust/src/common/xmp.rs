//! XMP packets (the XMP Specification, 2005, which PDF/X-4 cites; ISO 16684-1 says the same): well-formed XML whose
//! `rdf:RDF` holds `rdf:Description` elements, read into properties with their namespace URIs, plus every prefix the
//! packet used for each namespace, since PDF/X fixes some prefixes.
//!
//! Values are text, arrays (`rdf:Alt`, `rdf:Bag`, `rdf:Seq`, whose items may carry `xml:lang`) and structures. A
//! property qualified through `rdf:value` reads as its value. Anything else RDF/XML allows but XMP does not, such as
//! `rdf:parseType="Literal"` or two values in one property, is an error.

use roxmltree::{Document, Node, NodeType, ParsingOptions};

/// The RDF namespace.
pub(crate) const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
/// The XML namespace, which `xml:lang` is in.
const XML: &str = "http://www.w3.org/XML/1998/namespace";

/// A parsed packet.
#[derive(Debug, PartialEq)]
pub(crate) struct Packet {
    /// The top-level properties, from every `rdf:Description`, in order.
    pub(crate) properties: Vec<Property>,
    /// Each `(prefix, namespace)` the packet's element and attribute names used, without repeats.
    pub(crate) prefixes: Vec<(String, String)>,
}

/// One property: its namespace URI, local name and value.
#[derive(Debug, PartialEq)]
pub(crate) struct Property {
    pub(crate) ns: String,
    pub(crate) name: String,
    pub(crate) value: Value,
}

/// An XMP value.
#[derive(Debug, PartialEq)]
pub(crate) enum Value {
    Text(String),
    /// An array's kind and its items, each with its `xml:lang`, if any.
    Array(Kind, Vec<(Option<String>, Value)>),
    Struct(Vec<Property>),
}

/// The three kinds of XMP array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Alt,
    Bag,
    Seq,
}

/// Why a packet could not be read.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    /// The bytes are not UTF-8, UTF-16 or UTF-32 text.
    Encoding,
    /// Not well-formed XML: the parser's message.
    Xml(String),
    /// Well-formed XML, but not XMP's form of RDF.
    Rdf(&'static str),
}

impl Packet {
    /// The value of the top-level property `name` in namespace `ns`.
    pub(crate) fn get(&self, ns: &str, name: &str) -> Option<&Value> {
        self.properties
            .iter()
            .find(|p| p.ns == ns && p.name == name)
            .map(|p| &p.value)
    }
}

impl Value {
    /// The text of a simple value, or of a language alternative's `x-default` item (its first item when none is).
    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            Self::Array(Kind::Alt, items) => items
                .iter()
                .find(|(lang, _)| lang.as_deref() == Some("x-default"))
                .or_else(|| items.first())
                .and_then(|(_, v)| v.text()),
            _ => None,
        }
    }
}

/// The packet's text: UTF-8 unless a byte order mark says UTF-16 or UTF-32.
fn decode(bytes: &[u8]) -> Option<String> {
    match bytes {
        [0, 0, 0xFE, 0xFF, rest @ ..] => utf32(units(rest, 4, true)?),
        [0xFF, 0xFE, 0, 0, rest @ ..] => utf32(units(rest, 4, false)?),
        [0xFE, 0xFF, rest @ ..] => utf16(units(rest, 2, true)?),
        [0xFF, 0xFE, rest @ ..] => utf16(units(rest, 2, false)?),
        _ => String::from_utf8(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes).to_vec()).ok(),
    }
}

/// `bytes` as code units `width` bytes wide, or `None` when a unit is cut short.
fn units(bytes: &[u8], width: usize, big: bool) -> Option<Vec<u32>> {
    let chunks = bytes.chunks_exact(width);
    if !chunks.remainder().is_empty() {
        return None;
    }
    let fold = |n: u32, b: &u8| (n << 8) | u32::from(*b);
    Some(
        chunks
            .map(|c| {
                if big {
                    c.iter().fold(0, fold)
                } else {
                    c.iter().rev().fold(0, fold)
                }
            })
            .collect(),
    )
}

fn utf16(units: Vec<u32>) -> Option<String> {
    // Units two bytes wide always fit in a u16.
    let units = units.into_iter().map(|u| u16::try_from(u).unwrap_or(0xDC00));
    char::decode_utf16(units).collect::<Result<String, _>>().ok()
}

fn utf32(units: Vec<u32>) -> Option<String> {
    units.into_iter().map(char::from_u32).collect()
}

/// Parse a packet's bytes.
pub(crate) fn parse(bytes: &[u8]) -> Result<Packet, Error> {
    let text = decode(bytes).ok_or(Error::Encoding)?;
    let options = ParsingOptions {
        allow_dtd: false,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(&text, options).map_err(|e| Error::Xml(e.to_string()))?;
    let rdf = doc
        .descendants()
        .find(|n| is(*n, RDF, "RDF"))
        .ok_or(Error::Rdf("no rdf:RDF element"))?;
    let mut properties = Vec::new();
    for description in elements(rdf) {
        if !is(description, RDF, "Description") {
            return Err(Error::Rdf("rdf:RDF holds something other than rdf:Description"));
        }
        properties.extend(fields(description)?);
    }
    let mut prefixes = Vec::new();
    for node in doc.descendants().filter(Node::is_element) {
        let qnames = std::iter::once((node.tag_name().namespace(), element_qname(&text, node))).chain(
            node.attributes()
                .map(|a| (a.namespace(), text.get(a.range_qname()).unwrap_or_default())),
        );
        for (ns, qname) in qnames {
            if let (Some(ns), Some((prefix, _))) = (ns, qname.split_once(':')) {
                let pair = (prefix.to_owned(), ns.to_owned());
                if !prefixes.contains(&pair) {
                    prefixes.push(pair);
                }
            }
        }
    }
    Ok(Packet { properties, prefixes })
}

/// An element's name as written, from its start tag.
fn element_qname<'a>(text: &'a str, node: Node<'_, '_>) -> &'a str {
    let tag = text.get(node.range().start + 1..).unwrap_or_default();
    tag.split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or_default()
}

fn is(node: Node<'_, '_>, ns: &str, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(ns) && node.tag_name().name() == name
}

fn elements<'a, 'i>(node: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(Node::is_element)
}

/// The properties a node carries: its attributes outside the RDF and XML namespaces, then its child elements.
fn fields(node: Node<'_, '_>) -> Result<Vec<Property>, Error> {
    let mut out = Vec::new();
    for attr in node.attributes() {
        match attr.namespace() {
            Some(RDF | XML) => {}
            Some(ns) => out.push(Property {
                ns: ns.to_owned(),
                name: attr.name().to_owned(),
                value: Value::Text(attr.value().to_owned()),
            }),
            None => return Err(Error::Rdf("a property attribute has no namespace")),
        }
    }
    for child in elements(node) {
        out.push(property(child)?);
    }
    Ok(out)
}

fn property(node: Node<'_, '_>) -> Result<Property, Error> {
    let ns = node
        .tag_name()
        .namespace()
        .ok_or(Error::Rdf("a property element has no namespace"))?;
    Ok(Property {
        ns: ns.to_owned(),
        name: node.tag_name().name().to_owned(),
        value: value(node)?,
    })
}

/// The value of a property element, or of an array item.
fn value(node: Node<'_, '_>) -> Result<Value, Error> {
    let children: Vec<Node<'_, '_>> = elements(node).collect();
    let text: String = node
        .children()
        .filter(|n| n.node_type() == NodeType::Text)
        .filter_map(|n| n.text())
        .collect();
    if !children.is_empty() && !text.trim().is_empty() {
        return Err(Error::Rdf("a property mixes text and elements"));
    }
    if let Some(resource) = node.attribute((RDF, "resource")) {
        return if children.is_empty() {
            Ok(Value::Text(resource.to_owned()))
        } else {
            Err(Error::Rdf("rdf:resource on an element with content"))
        };
    }
    match node.attribute((RDF, "parseType")) {
        Some("Resource") => return Ok(Value::Struct(fields(node)?)),
        Some(_) => return Err(Error::Rdf("an rdf:parseType other than Resource")),
        None => {}
    }
    let [child] = children.as_slice() else {
        if !children.is_empty() {
            return Err(Error::Rdf("a property holds more than one element"));
        }
        let fields = fields(node)?;
        return Ok(if fields.is_empty() {
            Value::Text(text)
        } else {
            Value::Struct(fields)
        });
    };
    let kind = match child.tag_name().name() {
        "Alt" => Kind::Alt,
        "Bag" => Kind::Bag,
        "Seq" => Kind::Seq,
        "Description" if child.tag_name().namespace() == Some(RDF) => {
            let mut fields = fields(*child)?;
            return Ok(match fields.iter().position(|p| p.ns == RDF && p.name == "value") {
                Some(i) => fields.swap_remove(i).value,
                None => Value::Struct(fields),
            });
        }
        _ => {
            return Err(Error::Rdf(
                "a property holds an element that is not an array or a description",
            ));
        }
    };
    if child.tag_name().namespace() != Some(RDF) {
        return Err(Error::Rdf(
            "a property holds an element that is not an array or a description",
        ));
    }
    let mut items = Vec::new();
    for li in elements(*child) {
        if !is(li, RDF, "li") {
            return Err(Error::Rdf("an array holds something other than rdf:li"));
        }
        items.push((li.attribute((XML, "lang")).map(str::to_owned), value(li)?));
    }
    Ok(Value::Array(kind, items))
}

/// The attribute names in each `<?xpacket begin=...?>` header of a packet's bytes, as written.
pub(crate) fn header_attributes(bytes: &[u8]) -> Vec<Vec<String>> {
    let text = String::from_utf8_lossy(bytes);
    text.split("<?xpacket ")
        .skip(1)
        .filter_map(|rest| rest.split_once("?>").map(|(inside, _)| inside))
        .filter(|inside| inside.trim_start().starts_with("begin"))
        .map(|inside| {
            inside
                .split(['"', '\''])
                .step_by(2)
                .filter_map(|part| part.trim().strip_suffix('='))
                .map(|name| name.trim().to_owned())
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const HEAD: &str = r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#;
    const TAIL: &str = r#"</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
    const DC: &str = "http://purl.org/dc/elements/1.1/";
    const XMP: &str = "http://ns.adobe.com/xap/1.0/";

    fn packet(body: &str) -> Result<Packet, Error> {
        parse(format!("{HEAD}{body}{TAIL}").as_bytes())
    }

    fn text(t: &str) -> Value {
        Value::Text(t.to_owned())
    }

    #[test]
    fn simple_properties_as_elements_and_attributes() {
        let p = packet(
            r#"<rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:CreatorTool="t">
               <xmp:CreateDate>2026</xmp:CreateDate><xmp:Ref rdf:resource="uuid:1"/></rdf:Description>"#,
        )
        .unwrap();
        assert_eq!(p.get(XMP, "CreatorTool"), Some(&text("t")));
        assert_eq!(p.get(XMP, "CreateDate").and_then(Value::text), Some("2026"));
        assert_eq!(p.get(XMP, "Ref"), Some(&text("uuid:1")));
        assert_eq!(p.get(XMP, "Nothing"), None);
        let mut used = p.prefixes.clone();
        used.sort();
        let expect = [("rdf", RDF), ("x", "adobe:ns:meta/"), ("xmp", XMP)].map(|(a, b)| (a.to_owned(), b.to_owned()));
        assert_eq!(used, expect);
    }

    #[test]
    fn arrays_structures_and_qualifiers() {
        let p = packet(
            r#"<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:s="urn:s">
               <dc:title><rdf:Alt><rdf:li xml:lang="en">E</rdf:li><rdf:li xml:lang="x-default">D</rdf:li></rdf:Alt></dc:title>
               <dc:creator><rdf:Seq><rdf:li>A</rdf:li></rdf:Seq></dc:creator>
               <dc:subject><rdf:Bag/></dc:subject>
               <dc:rights><rdf:Alt><rdf:li xml:lang="de">R</rdf:li></rdf:Alt></dc:rights>
               <s:a rdf:parseType="Resource"><s:b>1</s:b></s:a>
               <s:c><rdf:Description s:d="2"/></s:c>
               <s:e s:f="3"/>
               <s:g><rdf:Description><rdf:value>v</rdf:value><s:q>x</s:q></rdf:Description></s:g>
               </rdf:Description>"#,
        )
        .unwrap();
        assert_eq!(p.get(DC, "title").and_then(Value::text), Some("D"));
        assert_eq!(p.get(DC, "rights").and_then(Value::text), Some("R"));
        assert_eq!(
            p.get(DC, "creator"),
            Some(&Value::Array(Kind::Seq, vec![(None, text("A"))]))
        );
        assert_eq!(p.get(DC, "creator").and_then(Value::text), None);
        assert_eq!(p.get(DC, "subject"), Some(&Value::Array(Kind::Bag, vec![])));
        assert_eq!(p.get(DC, "subject").and_then(Value::text), None);
        let field = |n: &str, v: &str| Property {
            ns: "urn:s".to_owned(),
            name: n.to_owned(),
            value: text(v),
        };
        assert_eq!(p.get("urn:s", "a"), Some(&Value::Struct(vec![field("b", "1")])));
        assert_eq!(p.get("urn:s", "c"), Some(&Value::Struct(vec![field("d", "2")])));
        assert_eq!(p.get("urn:s", "e"), Some(&Value::Struct(vec![field("f", "3")])));
        assert_eq!(p.get("urn:s", "g"), Some(&text("v")));
        assert!(p.prefixes.contains(&("xml".to_owned(), XML.to_owned())));
    }

    #[test]
    fn a_prefix_is_recorded_as_written() {
        let p = packet(
            r#"<rdf:Description xmlns:d="http://purl.org/dc/elements/1.1/"><d:format>x</d:format></rdf:Description>"#,
        )
        .unwrap();
        assert!(p.prefixes.contains(&("d".to_owned(), DC.to_owned())));
        let default =
            packet(r#"<rdf:Description><format xmlns="http://purl.org/dc/elements/1.1/">x</format></rdf:Description>"#)
                .unwrap();
        assert_eq!(default.get(DC, "format"), Some(&text("x")));
        assert!(!default.prefixes.iter().any(|(_, ns)| ns == DC));
    }

    #[test]
    fn what_xmp_does_not_allow() {
        let rdf = |body: &str| packet(body).unwrap_err();
        assert_eq!(
            rdf("<rdf:Bag/>"),
            Error::Rdf("rdf:RDF holds something other than rdf:Description")
        );
        assert_eq!(
            rdf(r#"<rdf:Description a="1"/>"#),
            Error::Rdf("a property attribute has no namespace")
        );
        assert_eq!(
            rdf("<rdf:Description><p/></rdf:Description>"),
            Error::Rdf("a property element has no namespace")
        );
        let s = |inner: &str| {
            rdf(&format!(
                r#"<rdf:Description xmlns:s="urn:s">{inner}</rdf:Description>"#
            ))
        };
        assert_eq!(
            s("<s:p>t<s:q/></s:p>"),
            Error::Rdf("a property mixes text and elements")
        );
        assert_eq!(
            s(r#"<s:p rdf:resource="u"><s:q/></s:p>"#),
            Error::Rdf("rdf:resource on an element with content")
        );
        assert_eq!(
            s(r#"<s:p rdf:parseType="Literal"/>"#),
            Error::Rdf("an rdf:parseType other than Resource")
        );
        assert_eq!(
            s("<s:p><s:q/><s:r/></s:p>"),
            Error::Rdf("a property holds more than one element")
        );
        let not = Error::Rdf("a property holds an element that is not an array or a description");
        assert_eq!(s("<s:p><s:q/></s:p>"), not);
        assert_eq!(s("<s:p><s:Alt/></s:p>"), not);
        assert_eq!(
            s("<s:p><rdf:Seq><s:li/></rdf:Seq></s:p>"),
            Error::Rdf("an array holds something other than rdf:li")
        );
        assert_eq!(
            s("<s:p><rdf:Seq><rdf:li><s:q/><s:r/></rdf:li></rdf:Seq></s:p>"),
            Error::Rdf("a property holds more than one element")
        );
        assert_eq!(
            s(r#"<s:p rdf:parseType="Resource" b="1"/>"#),
            Error::Rdf("a property attribute has no namespace")
        );
        assert_eq!(
            s(r#"<s:p><rdf:Description b="1"/></s:p>"#),
            Error::Rdf("a property attribute has no namespace")
        );
        assert_eq!(
            s(r#"<s:p b="1"/>"#),
            Error::Rdf("a property attribute has no namespace")
        );
        assert_eq!(parse(b"<a/>").unwrap_err(), Error::Rdf("no rdf:RDF element"));
        assert!(matches!(parse(b"<a>").unwrap_err(), Error::Xml(_)));
        assert!(matches!(parse(b"<!DOCTYPE a []><a/>").unwrap_err(), Error::Xml(_)));
        assert_eq!(parse(b"\xC3(").unwrap_err(), Error::Encoding);
    }

    #[test]
    fn encodings() {
        let body = format!(r#"{HEAD}<rdf:Description xmlns:s="urn:s" s:a="é😀"/>{TAIL}"#);
        let expect = parse(body.as_bytes()).unwrap();
        let utf16 = |big: bool| -> Vec<u8> {
            let mut out: Vec<u8> = if big { vec![0xFE, 0xFF] } else { vec![0xFF, 0xFE] };
            out.extend(
                body.encode_utf16()
                    .flat_map(|u| if big { u.to_be_bytes() } else { u.to_le_bytes() }),
            );
            out
        };
        let utf32 = |big: bool| -> Vec<u8> {
            let mut out: Vec<u8> = if big {
                vec![0, 0, 0xFE, 0xFF]
            } else {
                vec![0xFF, 0xFE, 0, 0]
            };
            out.extend(body.chars().flat_map(|c| {
                if big {
                    u32::from(c).to_be_bytes()
                } else {
                    u32::from(c).to_le_bytes()
                }
            }));
            out
        };
        for bytes in [
            utf16(true),
            utf16(false),
            utf32(true),
            utf32(false),
            [b"\xEF\xBB\xBF".as_slice(), body.as_bytes()].concat(),
        ] {
            assert_eq!(parse(&bytes).as_ref(), Ok(&expect));
        }
        assert_eq!(parse(b"\xFE\xFF\x00").unwrap_err(), Error::Encoding, "odd length");
        assert_eq!(
            parse(b"\xFE\xFF\xD8\x00").unwrap_err(),
            Error::Encoding,
            "lone surrogate"
        );
        assert_eq!(
            parse(b"\x00\x00\xFE\xFF\x00\x11\x00\x00").unwrap_err(),
            Error::Encoding,
            "beyond Unicode"
        );
        assert_eq!(parse(b"\x00\x00\xFE\xFF\x00").unwrap_err(), Error::Encoding, "ragged");
        assert_eq!(
            parse(b"\xFF\xFE\x00\xD8").unwrap_err(),
            Error::Encoding,
            "lone surrogate, little-endian"
        );
        assert_eq!(
            parse(b"\xFF\xFE\x00").unwrap_err(),
            Error::Encoding,
            "odd length, little-endian"
        );
        assert_eq!(
            parse(b"\xFF\xFE\x00\x00\x00").unwrap_err(),
            Error::Encoding,
            "ragged, little-endian"
        );
    }

    #[test]
    fn packet_headers() {
        let bytes = br#"<?xpacket begin="" id='x' bytes="5" encoding="UTF-8"?><a/><?xpacket end="w"?><?xpacket begin="" id="y"?>"#;
        assert_eq!(
            header_attributes(bytes),
            vec![vec!["begin", "id", "bytes", "encoding"], vec!["begin", "id"]]
        );
        assert_eq!(header_attributes(b"<?xpacket begin=\"\""), Vec::<Vec<String>>::new());
    }
}
