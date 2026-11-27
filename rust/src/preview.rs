//! Page previews: each page rendered to a PNG by `hayro`, optionally with the page-box guides drawn on it.
//!
//! Previews read the file with `hayro`'s own lenient parser, never the core's: a damaged file still gets a picture, and
//! nothing here judges a file. The whole `MediaBox` is rendered, as Ghostscript renders it, though `hayro` leaves blank
//! whatever lies outside the `CropBox`. The guides are the page's own `TrimBox` and `BleedBox` (and a safe area inset from the
//! trim), drawn from a small PDF of rectangles and labels rendered over the page, so they turn with a rotated page.

use std::panic::{AssertUnwindSafe, catch_unwind};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::object::Rect;
use hayro::hayro_syntax::page::{Page, Rotation};
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::kurbo::Affine;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings, render_into};

const PT_PER_IN: f64 = 72.0;
const MM_PER_IN: f64 = 25.4;

/// The guide colours, as RGB: the finished edge matters most, so it is the boldest.
pub const TRIM_RGB: [u8; 3] = [214, 34, 34];
/// The bleed edge.
pub const BLEED_RGB: [u8; 3] = [0, 110, 210];
/// The safe area.
pub const SAFE_RGB: [u8; 3] = [0, 150, 70];

/// The page-box guides to draw: `safe_mm`, when given, adds a safe-area guide that far inside the trim edge.
#[derive(Clone, Copy, Debug)]
pub struct Guides {
    /// The safe area's inset from the trim edge, in millimetres.
    pub safe_mm: Option<f64>,
}

/// Why a file could not be rendered.
#[derive(Debug, PartialEq)]
pub enum RenderError {
    /// `hayro` cannot read the file: why.
    Unreadable(String),
    /// A page at the resolution asked for would be empty, or wider or taller than 65,535 pixels: which.
    Size(String),
}

/// Each page of the PDF `bytes` as a PNG at `dpi`, with `guides` drawn when given.
///
/// # Errors
/// When `hayro` cannot read the file, or a page at `dpi` would be empty or too large to render.
pub fn render(bytes: &[u8], dpi: f64, guides: Option<Guides>) -> Result<Vec<Vec<u8>>, RenderError> {
    let pdf = Pdf::new(bytes.to_vec()).map_err(|e| {
        let why = match e {
            LoadPdfError::Decryption(_) => "its encryption cannot be opened",
            LoadPdfError::Invalid => "the renderer cannot read it",
        };
        RenderError::Unreadable(format!("the file cannot be rendered: {why}"))
    })?;
    let cache = RenderCache::new();
    let mut out = Vec::new();
    for (index, page) in pdf.pages().iter().enumerate() {
        let png = guarded(index + 1, || render_page(page, &cache, dpi, guides)).and_then(|png| {
            png.ok_or_else(|| {
                RenderError::Size(format!(
                    "page {} at {dpi} dpi cannot be rendered: it would be empty, or wider or taller than 65,535 pixels",
                    index + 1
                ))
            })
        })?;
        out.push(png);
    }
    Ok(out)
}

/// Run `render` for page `number`, turning a panic inside `hayro` into an error, so that nothing panics into the
/// caller on a hostile file. Unwinding is safe here: the page's pixmap and context die with the closure.
fn guarded<T>(number: usize, render: impl FnOnce() -> T) -> Result<T, RenderError> {
    catch_unwind(AssertUnwindSafe(render)).map_err(|panic| {
        let reason = panic
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        RenderError::Unreadable(if reason.is_empty() {
            format!("page {number} cannot be rendered")
        } else {
            format!("page {number} cannot be rendered: {reason}")
        })
    })
}

/// `page` as a PNG at `dpi` on white, with `guides`; `None` when it would be empty or too large for a pixmap, the one
/// case in which encoding a PNG into memory fails too.
fn render_page<'a>(page: &'a Page<'a>, cache: &RenderCache<'a>, dpi: f64, guides: Option<Guides>) -> Option<Vec<u8>> {
    let scale = dpi / PT_PER_IN;
    let media = page.media_box();
    let (width, height) = match page.rotation() {
        Rotation::None | Rotation::Flipped => (media.x1 - media.x0, media.y1 - media.y0),
        Rotation::Horizontal | Rotation::FlippedHorizontal => (media.y1 - media.y0, media.x1 - media.x0),
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a saturating cast, then checked against u16"
    )]
    let pixels = |pt: f64| u16::try_from((pt * scale).round() as i64).ok().filter(|px| *px > 0);
    let (w, h) = pixels(width).zip(pixels(height))?;
    let transform = to_pixels(media, page.rotation(), scale);
    let mut ctx = RenderContext::new(w, h);
    render_into(
        page,
        cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &mut ctx,
        transform,
    );
    if let Some(guides) = guides {
        draw_guides(&mut ctx, page, guides, dpi, transform);
    }
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render_with(
        &mut pixmap,
        &mut Resources::default(),
        RasterizerSettings {
            target_init: TargetInit::Clear(WHITE),
            ..Default::default()
        },
    );
    pixmap.into_png().ok()
}

/// Page space to pixels, y downwards: the `MediaBox` at `scale` pixels a point, turned clockwise by the page's rotation.
fn to_pixels(media: Rect, rotation: Rotation, s: f64) -> Affine {
    let Rect { x0, y0, x1, y1 } = media;
    Affine::new(match rotation {
        Rotation::None => [s, 0.0, 0.0, -s, -x0 * s, y1 * s],
        Rotation::Horizontal => [0.0, s, s, 0.0, -y0 * s, -x0 * s],
        Rotation::Flipped => [-s, 0.0, 0.0, s, x1 * s, -y0 * s],
        Rotation::FlippedHorizontal => [0.0, -s, -s, 0.0, y1 * s, x1 * s],
    })
}

/// Draw `page`'s bleed (when it differs from the `MediaBox`), trim and safe-area guides into `ctx`, whose page goes
/// through `transform` to pixels.
fn draw_guides(ctx: &mut RenderContext, page: &Page<'_>, guides: Guides, dpi: f64, transform: Affine) {
    let media = page.media_box();
    let own = |key: &[u8]| page.raw().get::<Rect>(key);
    let line = (dpi / 150.0).round().max(1.0);
    let mut boxes = Vec::new();
    if let Some(bleed) = own(b"BleedBox").filter(|b| {
        [b.x0, b.y0, b.x1, b.y1].map(f64::to_bits) != [media.x0, media.y0, media.x1, media.y1].map(f64::to_bits)
    }) {
        boxes.push((bleed, BLEED_RGB, line, "bleed"));
    }
    if let Some(trim) = own(b"TrimBox") {
        boxes.push((trim, TRIM_RGB, line + 1.0, "trim"));
        if let Some(mm) = guides.safe_mm {
            let d = mm / MM_PER_IN * PT_PER_IN;
            let safe = Rect::new(trim.x0 + d, trim.y0 + d, trim.x1 - d, trim.y1 - d);
            boxes.push((safe, SAFE_RGB, line, "safe"));
        }
    }
    // Each box in whole pixels, as seen: `[left, top, right, bottom]`, the right and bottom pixels included.
    let pixel_boxes: Vec<_> = boxes
        .into_iter()
        .map(|(r, rgb, width, label)| {
            let seen = transform.transform_rect_bbox(hayro::kurbo::Rect::new(r.x0, r.y0, r.x1, r.y1));
            let edges = [seen.x0, seen.y0, seen.x1, seen.y1].map(f64::round);
            (edges, rgb, width, label)
        })
        .collect();
    let height = f64::from(ctx.height());
    let sheet = overlay(f64::from(ctx.width()), height, dpi, &pixel_boxes);
    let crisp = RenderSettings {
        aliasing_threshold: Some(128),
        ..RenderSettings::default()
    };
    // The overlay is a PDF of our own, which always reads (a test holds it).
    let parsed = Pdf::new(sheet).ok();
    let flip = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, height]);
    for sheet in parsed.iter().filter_map(|pdf| pdf.pages().first()) {
        render_into(
            sheet,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &crisp,
            ctx,
            flip,
        );
    }
}

/// A one-page PDF `width` by `height` pixels, a unit a pixel and y upwards, holding each guide: its outline `line`
/// pixels wide inside the box, and its label just inside the top-left corner, white-haloed so it reads over artwork.
fn overlay(width: f64, height: f64, dpi: f64, boxes: &[([f64; 4], [u8; 3], f64, &str)]) -> Vec<u8> {
    let font = (dpi / 15.0).round().max(10.0);
    let mut content = Vec::new();
    for ([left, top, right, bottom], [red, green, blue], line, label) in boxes {
        let colour = format!(
            "{} {} {}",
            f64::from(*red) / 255.0,
            f64::from(*green) / 255.0,
            f64::from(*blue) / 255.0
        );
        // The outline's middle runs half a line inside the outermost pixels, so it covers whole pixels.
        let (x0, x1) = (left + line / 2.0, right + 1.0 - line / 2.0);
        let (y0, y1) = (height - bottom - 1.0 + line / 2.0, height - top - line / 2.0);
        content.push(format!("{colour} RG {line} w {x0} {y0} {} {} re S", x1 - x0, y1 - y0));
        let (x, y) = (left + line + 2.0, height - (top + line + 1.0 + font * 0.8));
        // The halo's show moves the text position on, so the label itself starts again from the same place.
        content.push(format!(
            "BT /F {font} Tf 1 0 0 1 {x} {y} Tm 1 1 1 RG {} w 1 Tr ({label}) Tj \
             1 0 0 1 {x} {y} Tm {colour} rg 0 Tr ({label}) Tj ET",
            2.0 * line
        ));
    }
    let content = content.join("\n");
    one_page(
        &format!(
            "/MediaBox [0 0 {width} {height}] \
             /Resources << /Font << /F << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>"
        ),
        &content,
        "",
    )
}

/// A one-page PDF whose page dictionary has `entries` besides its type, parent and contents, whose content is
/// `content`, and whose trailer has `trailer` besides its size and root.
fn one_page(entries: &str, content: &str, trailer: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R {entries} >>"),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).into_bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
    for offset in offsets {
        pdf.extend(format!("{offset:010} 00000 n \n").into_bytes());
    }
    let size = objects.len() + 1;
    pdf.extend(format!("trailer\n<< /Size {size} /Root 1 0 R {trailer} >>\nstartxref\n{xref}\n%%EOF\n").into_bytes());
    pdf
}

#[cfg(test)]
mod tests {
    use hayro::vello_cpu::Pixmap;

    use super::*;

    /// The colour of every pixel of `png`, row by row.
    fn pixels(png: &[u8]) -> (usize, usize, Vec<[u8; 3]>) {
        let map = Pixmap::from_png(std::io::Cursor::new(png)).unwrap();
        let rgb = map.data().iter().map(|p| [p.r, p.g, p.b]).collect();
        (usize::from(map.width()), usize::from(map.height()), rgb)
    }

    /// A page 300 by 200 points with `boxes` and `rotate`, painting blue where page space has its origin.
    fn page(boxes: &str, rotate: u32) -> Vec<u8> {
        one_page(
            &format!("/MediaBox [0 0 300 200] {boxes} /Rotate {rotate}"),
            "0 0 1 rg 0 0 50 50 re f",
            "",
        )
    }

    #[test]
    fn guides_come_from_the_pages_own_boxes() {
        let file = page("/BleedBox [10 10 290 190] /TrimBox [20 20 280 180]", 0);
        let [png] = render(&file, 72.0, Some(Guides { safe_mm: Some(5.0) }))
            .unwrap()
            .try_into()
            .unwrap();
        let (width, height, rgb) = pixels(&png);
        assert_eq!((width, height), (300, 200));
        let row: Vec<[u8; 3]> = rgb[100 * width..101 * width].to_vec();
        // 1 px bleed at 10, 2 px trim at 20, 1 px safe at 20 + 5 mm (14.17 pt, rounded).
        assert_eq!(&row[9..12], &[[255, 255, 255], BLEED_RGB, [255, 255, 255]]);
        assert_eq!(&row[19..23], &[[255, 255, 255], TRIM_RGB, TRIM_RGB, [255, 255, 255]]);
        assert_eq!(row[34], SAFE_RGB);
        assert_eq!(&row[278..282], &[[255, 255, 255], TRIM_RGB, TRIM_RGB, [255, 255, 255]]);
        // The page's own paint is there too, under the guides: blue at the bottom left.
        assert_eq!(rgb[195 * width + 5], [0, 0, 255]);
    }

    #[test]
    fn guides_are_left_out_when_not_asked_for_or_not_there() {
        let colours = |file: &[u8], guides| {
            let [png] = render(file, 72.0, guides).unwrap().try_into().unwrap();
            let rgb = pixels(&png).2;
            [BLEED_RGB, TRIM_RGB, SAFE_RGB].map(|c| rgb.contains(&c))
        };
        let full = page("/BleedBox [10 10 290 190] /TrimBox [20 20 280 180]", 0);
        assert_eq!(colours(&full, None), [false; 3]);
        let guides = Some(Guides { safe_mm: None });
        assert_eq!(colours(&full, guides), [true, true, false]);
        // A bleed box equal to the media box draws nothing, and without a trim box there is no trim or safe area.
        let bleed_is_media = page("/BleedBox [0 0 300 200]", 0);
        assert_eq!(
            colours(&bleed_is_media, Some(Guides { safe_mm: Some(3.0) })),
            [false; 3]
        );
    }

    #[test]
    fn a_rotated_page_turns_clockwise_with_its_guides() {
        // Where page space's origin (the blue square) lands, and the picture's size, for each /Rotate.
        for (rotate, size, corner) in [
            (0, (300, 200), (5, 195)),
            (90, (200, 300), (5, 5)),
            (180, (300, 200), (295, 5)),
            (270, (200, 300), (195, 295)),
        ] {
            let file = page("/TrimBox [20 20 280 180]", rotate);
            let [png] = render(&file, 72.0, Some(Guides { safe_mm: None }))
                .unwrap()
                .try_into()
                .unwrap();
            let (width, height, rgb) = pixels(&png);
            assert_eq!((width, height), size, "/Rotate {rotate}");
            assert_eq!(rgb[corner.1 * width + corner.0], [0, 0, 255], "/Rotate {rotate}");
            assert!(rgb.contains(&TRIM_RGB), "/Rotate {rotate}");
        }
    }

    #[test]
    fn a_file_or_page_that_cannot_be_rendered_says_why() {
        let unreadable = |e| matches!(e, RenderError::Unreadable(_));
        assert!(render(b"not a pdf", 72.0, None).is_err_and(unreadable));
        // Encryption hayro cannot open: a standard handler with no /ID to derive the key from.
        let locked = one_page(
            "/MediaBox [0 0 10 10]",
            "",
            "/Encrypt << /Filter /Standard /V 1 /R 2 /O <00> /U <00> /P -4 >>",
        );
        assert_eq!(
            render(&locked, 72.0, None),
            Err(RenderError::Unreadable(
                "the file cannot be rendered: its encryption cannot be opened".to_owned()
            ))
        );
        let size = |e| matches!(e, RenderError::Size(why) if why.starts_with("page 1 at "));
        assert!(render(&page("", 0), 100_000.0, None).is_err_and(size));
        assert!(render(&page("", 0), 0.0, None).is_err_and(size));
    }

    #[test]
    fn a_panic_while_rendering_is_an_error() {
        let failed = |number, panic: fn() -> ()| guarded(number, panic).unwrap_err();
        assert_eq!(
            failed(2, || panic!("bad glyph")),
            RenderError::Unreadable("page 2 cannot be rendered: bad glyph".to_owned())
        );
        let owned = || std::panic::panic_any(format!("index {}", 9));
        assert_eq!(
            failed(1, owned),
            RenderError::Unreadable("page 1 cannot be rendered: index 9".to_owned())
        );
        let silent = || std::panic::panic_any(7);
        assert_eq!(
            failed(1, silent),
            RenderError::Unreadable("page 1 cannot be rendered".to_owned())
        );
        assert_eq!(guarded(1, || 5), Ok(5));
    }

    #[test]
    fn the_overlay_is_a_pdf_hayro_reads() {
        let sheet = overlay(100.0, 50.0, 72.0, &[([1.0, 2.0, 90.0, 40.0], TRIM_RGB, 2.0, "trim")]);
        assert_eq!(Pdf::new(sheet).unwrap().pages().len(), 1);
    }
}
