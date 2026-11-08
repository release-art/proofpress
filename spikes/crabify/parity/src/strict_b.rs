use crate::strict::{Dict, Doc, Obj, content, filter_names};
use crate::{ID, M, MAX_DEPTH, Sink, hex, mul, short};
use serde_json::{Value, json};
use std::time::Instant;

const IMAGE: [&[u8]; 4] = [b"DCTDecode", b"JPXDecode", b"CCITTFaxDecode", b"JBIG2Decode"];

fn name(n: &[u8]) -> String {
    format!("/{}", String::from_utf8_lossy(n))
}

fn dict(d: &Dict, stream: bool) -> String {
    let mut e: Vec<(String, String)> = d.iter().filter(|(k, _)| !(stream && *k == b"Length")).map(|(k, v)| (name(k), canon(v))).collect();
    e.sort();
    let p: Vec<String> = e.into_iter().map(|(k, v)| format!("{k} {v}")).collect();
    format!("<<{}>>", p.join(" "))
}

fn canon(o: &Obj) -> String {
    match o {
        Obj::Null => "null".into(),
        Obj::Bool(b) => b.to_string(),
        Obj::Int(i) => format!("{:.4}", *i as f64),
        Obj::Real(r) => format!("{r:.4}"),
        Obj::Str(s) => format!("({})", hex(s)),
        Obj::Name(n) => name(n),
        Obj::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(" ")),
        Obj::Dict(d) => dict(d, false),
        Obj::Stream(d, ..) => dict(d, true),
        Obj::Ref(n, g) => format!("R{n}.{g}"),
    }
}

fn six(doc: &Doc, a: Option<Obj>) -> M {
    let Some(Obj::Array(a)) = a else { return ID };
    let v: Vec<f64> = a.iter().filter_map(|x| doc.resolve(x).ok()?.num()).collect();
    v.try_into().unwrap_or(ID)
}

fn walk(doc: &Doc, data: &[u8], res: Option<&Dict>, base: M, depth: usize, seen: &[Obj], out: &mut Sink) -> Result<(), String> {
    let mut ctm = base;
    let mut stack: Vec<M> = vec![];
    for op in content(data)? {
        match op.name.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(base),
            "cm" => {
                let v: Vec<f64> = op.args.iter().filter_map(|o| o.num()).collect();
                ctm = mul(&<M>::try_from(v).unwrap_or(ID), &ctm);
            }
            _ => {}
        }
        let text: Vec<String> = op.args.iter().map(canon).collect();
        out.line(&op.name, depth, &ctm, &text.join(" "));
        if op.name == "Do" && depth < MAX_DEPTH {
            let (Some(Obj::Name(n)), Some(r)) = (op.args.first(), res) else { continue };
            let Some(Obj::Dict(xo)) = doc.get(r, b"XObject")? else { continue };
            let Some(raw) = xo.get(n) else { continue };
            let form = doc.resolve(raw)?;
            let Obj::Stream(d, ..) = &*form else { continue };
            if doc.get(d, b"Subtype")? != Some(Obj::Name(b"Form".to_vec())) || seen.contains(raw) {
                continue;
            }
            let fctm = mul(&six(doc, doc.get(d, b"Matrix")?), &ctm);
            let own = doc.get(d, b"Resources")?;
            let own = own.as_ref().and_then(|o| o.dict()).filter(|d| !d.is_empty());
            let mut next = seen.to_vec();
            next.push(raw.clone());
            walk(doc, &doc.stream(&form)?, own.or(res), fctm, depth + 1, &next, out)?;
        }
    }
    Ok(())
}

fn nums(doc: &Doc, o: Option<&Obj>) -> Value {
    let Some(Obj::Array(a)) = o else { return Value::Null };
    json!(a.iter().filter_map(|x| doc.resolve(x).ok()?.num()).collect::<Vec<f64>>())
}

fn page_body(doc: &Doc, d: &Dict) -> Result<Vec<u8>, String> {
    Ok(match doc.get(d, b"Contents")? {
        Some(s @ Obj::Stream(..)) => doc.stream(&s)?,
        Some(Obj::Array(a)) => {
            let mut v = vec![];
            for x in &a {
                v.extend(doc.stream(&*doc.resolve(x)?)?);
                v.push(b' ');
            }
            v
        }
        _ => vec![],
    })
}

pub fn record(path: &str, keys: &[(u32, i32)], dump: Option<usize>) -> Value {
    let t0 = Instant::now();
    let doc = match Doc::open(std::fs::read(path).unwrap()) {
        Ok(d) => d,
        Err(e) => return json!({"open": false, "err": e}),
    };
    let pages = match doc.pages() {
        Ok(p) => p,
        Err(e) => return json!({"open": false, "err": e}),
    };
    let open_s = t0.elapsed().as_secs_f64();
    if let Some(i) = dump {
        let p = &pages[i];
        let res = p.resources.as_ref().and_then(|r| r.dict());
        walk(&doc, &page_body(&doc, &p.dict).unwrap_or_default(), res, ID, 0, &[], &mut Sink::new(true)).unwrap();
        return Value::Null;
    }
    let boxes: Vec<Value> = pages
        .iter()
        .map(|p| {
            let m = nums(&doc, p.media.as_ref());
            let c = nums(&doc, p.crop.as_ref());
            json!({"media": m, "crop": if c.is_null() { m.clone() } else { c },
                   "trim": nums(&doc, doc.get(&p.dict, b"TrimBox").ok().flatten().as_ref()),
                   "bleed": nums(&doc, doc.get(&p.dict, b"BleedBox").ok().flatten().as_ref()),
                   "rotate": p.rotate.as_ref().and_then(|r| r.num()).unwrap_or(0.0) as i64})
        })
        .collect();
    let root_obj = doc.get(&doc.trailer, b"Root").ok().flatten();
    let mut root: Vec<String> = root_obj.as_ref().and_then(|r| r.dict()).map(|d| d.keys().map(|k| name(k)).collect()).unwrap_or_default();
    root.sort();
    let t1 = Instant::now();
    let mut objects = serde_json::Map::new();
    let mut streams = serde_json::Map::new();
    for &(n, g) in keys {
        let key = format!("{n}.{g}");
        let Ok(o) = doc.load(n) else {
            objects.insert(key, "MISSING".into());
            continue;
        };
        objects.insert(key.clone(), short(canon(&o).as_bytes()).into());
        if let Obj::Stream(d, ..) = &*o {
            let names = filter_names(d, &|x| doc.resolve(x)).unwrap_or_default();
            let v = if names.iter().any(|f| IMAGE.contains(&f.as_slice())) {
                format!("raw:{}", short(doc.raw(&o)))
            } else {
                match doc.stream(&o) {
                    Ok(b) => short(&b),
                    Err(e) => format!("ERR:{e}"),
                }
            };
            streams.insert(key, v.into());
        }
    }
    let objects_s = t1.elapsed().as_secs_f64();
    let t2 = Instant::now();
    let walks: Vec<Value> = pages
        .iter()
        .map(|p| {
            let mut s = Sink::new(false);
            let res = p.resources.as_ref().and_then(|r| r.dict());
            match page_body(&doc, &p.dict).and_then(|b| walk(&doc, &b, res, ID, 0, &[], &mut s)) {
                Ok(()) => s.finish(),
                Err(e) => json!([s.n, format!("ERR:{e}")]),
            }
        })
        .collect();
    json!({"open": true, "open_s": open_s, "npages": pages.len(), "root": root, "pages": boxes, "objects": objects,
           "streams": streams, "objects_s": objects_s, "walks": walks, "walk_s": t2.elapsed().as_secs_f64()})
}
