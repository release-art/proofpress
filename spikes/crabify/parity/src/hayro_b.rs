use crate::{ID, M, MAX_DEPTH, Sink, hex, mul, short};
use hayro_syntax::Pdf;
use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, Rect, Stream};
use serde_json::{Value, json};
use std::time::Instant;

const IMAGE: [&str; 4] = ["DCTDecode", "JPXDecode", "CCITTFaxDecode", "JBIG2Decode"];

fn maybe(o: MaybeRef<Object<'_>>) -> String {
    match o {
        MaybeRef::Ref(r) => format!("R{}.{}", r.obj_number, r.gen_number),
        MaybeRef::NotRef(o) => canon(&o),
    }
}

fn dict(d: &Dict<'_>, stream: bool) -> String {
    let mut e: Vec<(String, String)> = d
        .entries()
        .filter(|(k, _)| !(stream && k.as_str() == "Length"))
        .filter_map(|(k, v)| {
            let s = maybe(v);
            (s != "null").then(|| (format!("/{}", k.as_str()), s)) // qpdf drops null-valued keys
        })
        .collect();
    e.sort();
    e.dedup_by(|a, b| a.0 == b.0);
    let parts: Vec<String> = e.into_iter().map(|(k, v)| format!("{k} {v}")).collect();
    format!("<<{}>>", parts.join(" "))
}

fn canon(o: &Object<'_>) -> String {
    match o {
        Object::Null(_) => "null".into(),
        Object::Boolean(b) => b.to_string(),
        Object::Number(n) => format!("{:.4}", n.as_f64()),
        Object::String(s) => format!("({})", hex(s.as_bytes())),
        Object::Name(n) => format!("/{}", n.as_str()),
        Object::Dict(d) => dict(d, false),
        Object::Array(a) => {
            let parts: Vec<String> = a.raw_iter().map(maybe).collect();
            format!("[{}]", parts.join(" "))
        }
        Object::Stream(s) => dict(s.dict(), true),
    }
}

fn rect(r: Rect) -> Value {
    json!([r.x0, r.y0, r.x1, r.y1])
}

fn six(a: Option<Array<'_>>) -> M {
    let v: Vec<f64> = a.map(|a| a.iter::<hayro_syntax::object::Number>().map(|n| n.as_f64()).collect()).unwrap_or_default();
    v.try_into().unwrap_or(ID)
}

fn walk(data: &[u8], res: Option<&Dict<'_>>, base: M, depth: usize, seen: &[ObjectIdentifier], out: &mut Sink) {
    let mut ctm = base;
    let mut stack: Vec<M> = vec![];
    let mut it = UntypedIter::new(data);
    while let Some(ins) = it.next() {
        let op = String::from_utf8_lossy(ins.operator.as_ref()).into_owned();
        let args: Vec<Object<'_>> = ins.operands().map(|o: &Object<'_>| o.clone()).collect();
        match op.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(base),
            "cm" => {
                let v: Vec<f64> = args.iter().filter_map(|o| o.clone().into_number().map(|n| n.as_f64())).collect();
                ctm = mul(&<M>::try_from(v).unwrap_or(ID), &ctm);
            }
            _ => {}
        }
        let text: Vec<String> = args.iter().map(canon).collect();
        out.line(&op, depth, &ctm, &text.join(" "));
        if op == "Do" && depth < MAX_DEPTH {
            let form = args.first().and_then(|a| a.clone().into_name()).and_then(|name: Name<'_>| {
                res?.get::<Dict<'_>>("XObject")?.get::<Stream<'_>>(name.as_str())
            });
            let Some(form) = form else { continue };
            if form.dict().get::<Name<'_>>("Subtype").is_none_or(|s| s.as_str() != "Form") {
                continue;
            }
            let id = form.obj_id();
            if seen.contains(&id) {
                continue;
            }
            let fctm = mul(&six(form.dict().get::<Array<'_>>("Matrix")), &ctm);
            let own = form.dict().get::<Dict<'_>>("Resources").filter(|d| !d.is_empty());
            let scope = own.as_ref().or(res);
            let mut next = seen.to_vec();
            next.push(id);
            if let Ok(body) = form.decoded_data() {
                walk(&body, scope, fctm, depth + 1, &next, out);
            }
        }
    }
}

/// The page's /Resources, its own or the nearest ancestor's: what pikepdf's page.obj shows after qpdf's push-down.
fn page_resources<'a>(p: &hayro_syntax::page::Page<'a>) -> Option<Dict<'a>> {
    let mut d = p.raw().clone();
    for _ in 0..64 {
        if let Some(r) = d.get::<Dict<'a>>("Resources") {
            return Some(r);
        }
        d = d.get::<Dict<'a>>("Parent")?;
    }
    None
}

pub fn record(path: &str, keys: &[(u32, i32)], dump: Option<usize>) -> Value {
    let t0 = Instant::now();
    let data = std::fs::read(path).unwrap();
    let pdf = match Pdf::new(data) {
        Ok(p) => p,
        Err(e) => return json!({"open": false, "err": format!("{e:?}")}),
    };
    let pages = pdf.pages();
    let open_s = t0.elapsed().as_secs_f64();
    if let Some(i) = dump {
        let p = &pages[i];
        let res = page_resources(p);
        walk(p.page_stream().unwrap_or(&[]), res.as_ref(), ID, 0, &[], &mut Sink::new(true));
        return Value::Null;
    }
    let boxes: Vec<Value> = pages
        .iter()
        .map(|p| {
            let rot = match p.rotation() {
                hayro_syntax::page::Rotation::None => 0,
                hayro_syntax::page::Rotation::Horizontal => 90,
                hayro_syntax::page::Rotation::Flipped => 180,
                hayro_syntax::page::Rotation::FlippedHorizontal => 270,
            };
            json!({"media": rect(p.media_box()), "crop": rect(p.crop_box()),
                   "trim": p.raw().get::<Rect>("TrimBox").map(rect), "bleed": p.raw().get::<Rect>("BleedBox").map(rect), "rotate": rot})
        })
        .collect();
    let xref = pdf.xref();
    let root: Vec<String> = xref
        .get::<Dict<'_>>(xref.root_id())
        .map(|d| {
            let mut k: Vec<String> = d.keys().map(|n| format!("/{}", n.as_str())).collect();
            k.sort();
            k
        })
        .unwrap_or_default();
    if let Ok(k) = std::env::var("DUMPOBJ") {
        let (n, g) = k.split_once('.').unwrap();
        let o = xref.get::<Object<'_>>(ObjectIdentifier::new(n.parse().unwrap(), g.parse().unwrap()));
        println!("{}", o.as_ref().map(canon).unwrap_or("MISSING".into()));
        if let Some(Object::Stream(s)) = &o {
            println!("decoded {:?} raw_len={} filters={:?}", s.decoded_data().map(|d| d.len()), s.raw_data().len(), s.filters());
        }
        std::process::exit(0);
    }
    let t1 = Instant::now();
    let mut objects = serde_json::Map::new();
    let mut streams = serde_json::Map::new();
    for &(n, g) in keys {
        let key = format!("{n}.{g}");
        let Some(o) = xref.get::<Object<'_>>(ObjectIdentifier::new(n, g)) else {
            objects.insert(key, "MISSING".into());
            continue;
        };
        objects.insert(key.clone(), short(canon(&o).as_bytes()).into());
        if let Object::Stream(s) = &o {
            let f = s.dict();
            let img = match f.get::<Object<'_>>("Filter") {
                Some(Object::Name(n)) => IMAGE.contains(&n.as_str()),
                Some(Object::Array(a)) => a.iter::<Name<'_>>().any(|n| IMAGE.contains(&n.as_str())),
                _ => false,
            };
            let v = if img {
                format!("raw:{}", short(&s.raw_data()))
            } else {
                match s.decoded_data() {
                    Ok(d) => short(&d),
                    Err(e) => format!("ERR:{e:?}"),
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
            let res = page_resources(p);
            walk(p.page_stream().unwrap_or(&[]), res.as_ref(), ID, 0, &[], &mut s);
            s.finish()
        })
        .collect();
    json!({"open": true, "open_s": open_s, "npages": pages.len(), "root": root, "pages": boxes, "objects": objects,
           "streams": streams, "objects_s": objects_s, "walks": walks, "walk_s": t2.elapsed().as_secs_f64()})
}
