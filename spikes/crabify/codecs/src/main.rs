//! codecs icc PROFILE.icc lcms|mox   CMYK grid on stdin as "c m y k" lines -> "L a b" 8-bit lines
//! codecs tac FILE.tif               peak C+M+Y+K as a percentage, as colour._peak_tac measures it
//! codecs image FILE.pdf N.G         decoded size, bands and per-band extrema of one image XObject
use hayro_syntax::Pdf;
use hayro_syntax::object::{Object, ObjectIdentifier, Stream};
use hayro_syntax::object::stream::ImageDecodeParams;
use std::io::{BufRead, Write};

fn grid() -> Vec<[u8; 4]> {
    std::io::stdin().lock().lines().map(|l| {
        let v: Vec<u8> = l.unwrap().split(' ').map(|x| x.parse().unwrap()).collect();
        [v[0], v[1], v[2], v[3]]
    }).collect()
}

fn icc(path: &str, engine: &str) {
    let data = std::fs::read(path).unwrap();
    let px = grid();
    let mut out = std::io::stdout().lock();
    if engine == "lcms" {
        use lcms2::*;
        let src = Profile::new_icc(&data).unwrap();
        let d50 = CIExyY { x: 0.3457, y: 0.3585, Y: 1.0 };
        let lab = Profile::new_lab2_context(GlobalContext::new(), &d50).unwrap();
        let t: Transform<[u8; 4], [u8; 3]> =
            Transform::new(&src, PixelFormat::CMYK_8, &lab, PixelFormat::Lab_8, Intent::RelativeColorimetric).unwrap();
        let mut dst = vec![[0u8; 3]; px.len()];
        t.transform_pixels(&px, &mut dst);
        for p in dst { writeln!(out, "{} {} {}", p[0], p[1], p[2]).unwrap(); }
    } else if engine == "mox32" {
        use moxcms::*;
        let src = ColorProfile::new_from_slice(&data).unwrap();
        let lab = ColorProfile::new_lab();
        let opts = TransformOptions { rendering_intent: RenderingIntent::RelativeColorimetric, ..Default::default() };
        let t = src.create_transform_f32(Layout::Rgba, &lab, Layout::Rgb, opts).unwrap();
        let flat: Vec<f32> = px.iter().flatten().map(|&v| v as f32 / 255.0).collect();
        let mut dst = vec![0f32; px.len() * 3];
        t.transform(&flat, &mut dst).unwrap();
        for p in dst.chunks(3) { writeln!(out, "{} {} {}", p[0], p[1], p[2]).unwrap(); }
    } else {
        use moxcms::*;
        let src = ColorProfile::new_from_slice(&data).unwrap();
        let lab = ColorProfile::new_lab();
        let opts = TransformOptions { rendering_intent: RenderingIntent::RelativeColorimetric, ..Default::default() };
        let t = src.create_transform_8bit(Layout::Rgba, &lab, Layout::Rgb, opts).unwrap();
        let flat: Vec<u8> = px.iter().flatten().copied().collect();
        let mut dst = vec![0u8; px.len() * 3];
        t.transform(&flat, &mut dst).unwrap();
        for p in dst.chunks(3) { writeln!(out, "{} {} {}", p[0], p[1], p[2]).unwrap(); }
    }
}

fn tac(path: &str) {
    let mut d = tiff::decoder::Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let t0 = std::time::Instant::now();
    let tiff::decoder::DecodingResult::U8(px) = d.read_image().unwrap() else { panic!("not 8-bit") };
    let peak = px.chunks_exact(4).map(|p| p.iter().map(|&v| v as u32).sum::<u32>()).max().unwrap_or(0);
    println!("{} {:.4}", peak as f64 / 255.0 * 100.0, t0.elapsed().as_secs_f64());
}

fn image(path: &str, key: &str) {
    let pdf = Pdf::new(std::fs::read(path).unwrap()).expect("open");
    let (n, g) = key.split_once('.').unwrap();
    let id = ObjectIdentifier::new(n.parse().unwrap(), g.parse().unwrap());
    let Some(Object::Stream(s)) = pdf.xref().get::<Object<'_>>(id) else { println!("{{\"err\":\"not a stream\"}}"); return };
    let s: Stream<'_> = s;
    let d = s.dict();
    let params = ImageDecodeParams {
        width: d.get::<u32>("Width").unwrap_or(0),
        height: d.get::<u32>("Height").unwrap_or(0),
        bpc: d.get::<u8>("BitsPerComponent"),
        ..Default::default()
    };
    let Ok(r) = s.decoded_image(&params) else { println!("{{\"err\":\"decode\"}}"); return };
    let Some(i) = r.image_data else { println!("{}", serde_json::json!({"len": r.data.len()})); return };
    let bands = match i.color_space {
        Some(hayro_syntax::object::stream::ImageColorSpace::Gray) => 1,
        Some(hayro_syntax::object::stream::ImageColorSpace::Rgb) => 3,
        Some(hayro_syntax::object::stream::ImageColorSpace::Cmyk) => 4,
        Some(hayro_syntax::object::stream::ImageColorSpace::Unknown(n)) => n as usize,
        None => 0,
    };
    let mut ext = vec![[255u8, 0u8]; bands];
    if i.bits_per_component == 8 && bands > 0 {
        for p in r.data.chunks_exact(bands) {
            for (e, &v) in ext.iter_mut().zip(p) { e[0] = e[0].min(v); e[1] = e[1].max(v); }
        }
    }
    println!("{}", serde_json::json!({"size": [i.width, i.height], "bands": bands, "bpc": i.bits_per_component, "ext": ext}));
}

/// crop.py on lopdf: one page, window as Media/CropBox, no trim/bleed/art/annots.
fn crop(path: &str, page: u32, b: [f32; 4], out: &str) {
    use lopdf::{Document, Object};
    let mut doc = Document::load(path).unwrap();
    let pages = doc.get_pages();
    let id = pages[&page];
    // Inherited attributes must be pulled down before the page leaves its parent.
    let mut carried = vec![];
    for key in [&b"Resources"[..], b"Rotate"] {
        let mut node = doc.get_dictionary(id).unwrap().clone();
        for _ in 0..64 {
            if let Ok(v) = node.get(key) { carried.push((key.to_vec(), v.clone())); break; }
            let Ok(parent) = node.get(b"Parent").and_then(|p| p.as_reference()) else { break };
            node = doc.get_dictionary(parent).unwrap().clone();
        }
    }
    let others: Vec<u32> = pages.keys().copied().filter(|&n| n != page).collect();
    doc.delete_pages(&others);
    let d = doc.get_dictionary_mut(id).unwrap();
    for (k, v) in carried { d.set(k, v); }
    for k in [&b"TrimBox"[..], b"BleedBox", b"ArtBox", b"Annots", b"StructParents"] { d.remove(k); }
    let window = Object::Array(b.iter().map(|&v| Object::Real(v)).collect());
    d.set("MediaBox", window.clone());
    d.set("CropBox", window);
    doc.prune_objects();
    doc.save(out).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a[1].as_str() {
        "icc" => icc(&a[2], &a[3]),
        "tac" => tac(&a[2]),
        "image" => image(&a[2], &a[3]),
        "crop" => crop(&a[2], a[3].parse().unwrap(), [a[4].parse().unwrap(), a[5].parse().unwrap(), a[6].parse().unwrap(), a[7].parse().unwrap()], &a[8]),
        _ => panic!(),
    }
}
