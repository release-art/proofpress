//! standalone render FILE.pdf PAGE DPI OUT.png     hayro in place of gs -sDEVICE=png16m
//! standalone pdfa FILE.pdf                        pdfpurr's PDF/A-2b verdict, to set against veraPDF
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a[1].as_str() {
        "render" => {
            let t = std::time::Instant::now();
            let pdf = Pdf::new(std::fs::read(&a[2]).unwrap()).unwrap();
            let page = &pdf.pages()[a[3].parse::<usize>().unwrap()];
            let s = a[4].parse::<f32>().unwrap() / 72.0;
            let px = PixmapSettings { x_scale: s, y_scale: s, ..Default::default() };
            let map = render(page, &RenderCache::new(), &InterpreterSettings::default(), &RenderSettings::default(), &px);
            std::fs::write(&a[5], map.into_png().unwrap()).unwrap();
            println!("{:.3}", t.elapsed().as_secs_f64());
        }
        "pdfa" => {
            let doc = pdfpurr::Document::open(&a[2]).unwrap();
            let r = doc.validate_pdfa(pdfpurr::standards::common::PdfALevel::A2b);
            println!("{}", r.is_compliant());
        }
        _ => panic!(),
    }
}
