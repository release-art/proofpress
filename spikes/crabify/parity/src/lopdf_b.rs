use crate::{ID, M, MAX_DEPTH, Sink, hex, mul, short};
use lopdf::{Dictionary, Document, Object, ObjectId, content::Content};
use serde_json::{Value, json};
use std::time::Instant;

const IMAGE: [&[u8]; 4] = [b"DCTDecode", b"JPXDecode", b"CCITTFaxDecode", b"JBIG2Decode"];

fn name(n: &[u8]) -> String {
    format!("/{}", String::from_utf8_lossy(n))
}

fn dict(d: &Dictionary, stream: bool) -> String {
    let mut e: Vec<(String, String)> = d
        .iter()
        .filter(|(k, v)| !matches!(v, Object::Null) && !(stream && *k == b"Length"))
        .map(|(k, v)| (name(k), canon(v)))
        .collect();
    e.sort();
    let parts: Vec<String> = e.into_iter().map(|(k, v)| format!("{k} {v}")).collect();
    format!("<<{}>>", parts.join(" "))
}

fn canon(o: &Object) -> String {
    match o {
        Object::Null => "null".into(),
        Object::Boolean(b) => b.to_string(),
        Object::Integer(i) => format!("{:.4}", *i as f64),
        Object::Real(r) => format!("{:.4}", *r as f64),
        Object::Name(n) => name(n),
        Object::String(s, _) => format!("({})", hex(s)),
        Object::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(" ")),
        Object::Dictionary(d) => dict(d, false),
        Object::Stream(s) => dict(&s.dict, true),
        Object::Reference((n, g)) => format!("R{n}.{g}"),
    }
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Object> {
    doc.dereference(o).ok().map(|x| x.1)
}

fn get<'a>(doc: &'a Document, d: &'a Dictionary, k: &[u8]) -> Option<&'a Object> {
    deref(doc, d.get(k).ok()?)
}

fn nums(doc: &Document, o: Option<&Object>) -> Option<Vec<f64>> {
    let v: Vec<f64> = o?.as_array().ok()?.iter().filter_map(|x| deref(doc, x)?.as_float().ok().map(|f| f as f64)).collect();
    Some(v)
}

fn inherited<'a>(doc: &'a Document, mut d: &'a Dictionary, k: &[u8]) -> Option<&'a Object> {
    for _ in 0..64 {
        if let Some(v) = get(doc, d, k) {
            return Some(v);
        }
        d = get(doc, d, b"Parent")?.as_dict().ok()?;
    }
    None
}

fn walk(doc: &Document, data: &[u8], res: Option<&Dictionary>, base: M, depth: usize, seen: &[ObjectId], out: &mut Sink) {
    let mut ctm = base;
    let mut stack: Vec<M> = vec![];
    let Ok(content) = Content::decode(data) else {
        out.line("PARSE-FAILED", depth, &ctm, "");
        return;
    };
    for op in content.operations {
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(base),
            "cm" => {
                let v: Vec<f64> = op.operands.iter().filter_map(|o| o.as_float().ok().map(|f| f as f64)).collect();
                ctm = mul(&<M>::try_from(v).unwrap_or(ID), &ctm);
            }
            _ => {}
        }
        let text: Vec<String> = op.operands.iter().map(canon).collect();
        out.line(&op.operator, depth, &ctm, &text.join(" "));
        if op.operator == "Do" && depth < MAX_DEPTH {
            let Some(Object::Name(n)) = op.operands.first() else { continue };
            let Some(xo) = res.and_then(|r| get(doc, r, b"XObject")).and_then(|x| x.as_dict().ok()) else { continue };
            let Ok(raw) = xo.get(n) else { continue };
            let id = raw.as_reference().ok();
            let Some(Object::Stream(form)) = deref(doc, raw) else { continue };
            if get(doc, &form.dict, b"Subtype").and_then(|s| s.as_name().ok()) != Some(&b"Form"[..]) {
                continue;
            }
            if id.is_some_and(|i| seen.contains(&i)) {
                continue;
            }
            let m = nums(doc, get(doc, &form.dict, b"Matrix")).and_then(|v| <M>::try_from(v).ok()).unwrap_or(ID);
            let fctm = mul(&m, &ctm);
            let own = get(doc, &form.dict, b"Resources").and_then(|r| r.as_dict().ok()).filter(|d| !d.is_empty());
            let mut next = seen.to_vec();
            next.extend(id);
            let body = form.decompressed_content().unwrap_or_else(|_| form.content.clone());
            walk(doc, &body, own.or(res), fctm, depth + 1, &next, out);
        }
    }
}

pub fn record(path: &str, keys: &[(u32, i32)], dump: Option<usize>) -> Value {
    let t0 = Instant::now();
    let doc = match Document::load(path) {
        Ok(d) => d,
        Err(e) => return json!({"open": false, "err": format!("{e}")}),
    };
    let ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let open_s = t0.elapsed().as_secs_f64();
    let body = |id: ObjectId| doc.get_page_content(id);
    let page_res = |d: &'_ Dictionary| inherited(&doc, d, b"Resources").and_then(|r| r.as_dict().ok()).cloned();
    if let Some(i) = dump {
        let d = doc.get_dictionary(ids[i]).unwrap();
        let res = page_res(d);
        walk(&doc, &body(ids[i]), res.as_ref(), ID, 0, &[], &mut Sink::new(true));
        return Value::Null;
    }
    let boxes: Vec<Value> = ids
        .iter()
        .map(|&id| {
            let d = doc.get_dictionary(id).unwrap();
            let media = nums(&doc, inherited(&doc, d, b"MediaBox"));
            let crop = nums(&doc, inherited(&doc, d, b"CropBox")).or_else(|| media.clone());
            let rot = inherited(&doc, d, b"Rotate").and_then(|r| r.as_i64().ok()).unwrap_or(0);
            json!({"media": media, "crop": crop, "trim": nums(&doc, get(&doc, d, b"TrimBox")),
                   "bleed": nums(&doc, get(&doc, d, b"BleedBox")), "rotate": rot})
        })
        .collect();
    let mut root: Vec<String> = doc.catalog().map(|c| c.iter().map(|(k, _)| name(k)).collect()).unwrap_or_default();
    root.sort();
    if let Ok(k) = std::env::var("DUMPOBJ") {
        let (n, g) = k.split_once('.').unwrap();
        let o = doc.get_object((n.parse().unwrap(), g.parse().unwrap()));
        println!("{}", o.map(canon).unwrap_or("MISSING".into()));
        std::process::exit(0);
    }
    let t1 = Instant::now();
    let mut objects = serde_json::Map::new();
    let mut streams = serde_json::Map::new();
    for &(n, g) in keys {
        let key = format!("{n}.{g}");
        let Ok(o) = doc.get_object((n, g as u16)) else {
            objects.insert(key, "MISSING".into());
            continue;
        };
        objects.insert(key.clone(), short(canon(o).as_bytes()).into());
        if let Object::Stream(s) = o {
            let filters = s.filters().unwrap_or_default();
            let v = if filters.iter().any(|f| IMAGE.contains(f)) {
                format!("raw:{}", short(&s.content))
            } else if filters.is_empty() {
                short(&s.content)
            } else {
                match s.decompressed_content() {
                    Ok(d) => short(&d),
                    Err(e) => format!("ERR:{e}"),
                }
            };
            streams.insert(key, v.into());
        }
    }
    let objects_s = t1.elapsed().as_secs_f64();
    let t2 = Instant::now();
    let walks: Vec<Value> = ids
        .iter()
        .map(|&id| {
            let mut s = Sink::new(false);
            let res = page_res(doc.get_dictionary(id).unwrap());
            walk(&doc, &body(id), res.as_ref(), ID, 0, &[], &mut s);
            s.finish()
        })
        .collect();
    json!({"open": true, "open_s": open_s, "npages": ids.len(), "root": root, "pages": boxes, "objects": objects,
           "streams": streams, "objects_s": objects_s, "walks": walks, "walk_s": t2.elapsed().as_secs_f64()})
}
