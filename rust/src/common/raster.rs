//! Decoding CMYK image samples as the Python print checks read them through Pillow: a JPEG with four components
//! (YCCK converted to CMYK with libjpeg's integer tables), and a JPEG 2000 file whose header gives four channels and
//! the enumerated CMYK space, each as interleaved 8-bit samples as stored (no Adobe inversion). Nothing else decodes,
//! and damaged data does not decode at all: Pillow refuses a truncated image.

use hayro_jpeg2000::{DecodeSettings, DecoderContext, Image};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use super::jpx;

/// Pillow refuses an image of more pixels than this: twice its default `MAX_IMAGE_PIXELS`.
const MAX_PIXELS: u64 = 2 * 89_478_485;
/// The enumerated CMYK colour space of a JPEG 2000 `colr` box (T.800, Table I.10).
const ENUMERATED_CMYK: u32 = 12;

fn too_large(width: u64, height: u64) -> bool {
    width.saturating_mul(height) > MAX_PIXELS
}

/// A CMYK or YCCK JPEG's samples, as CMYK.
pub(crate) fn jpeg_cmyk(data: &[u8]) -> Option<Vec<u8>> {
    let unlimited = DecoderOptions::default()
        .set_max_width(usize::MAX)
        .set_max_height(usize::MAX);
    let mut probe = JpegDecoder::new_with_options(ZCursor::new(data), unlimited);
    probe.decode_headers().ok()?;
    // Both are known once the headers are read.
    let input = probe.input_colorspace().unwrap_or(ColorSpace::Unknown);
    let (width, height) = probe.dimensions().unwrap_or_default();
    let wide = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    if !matches!(input, ColorSpace::CMYK | ColorSpace::YCCK) || too_large(wide(width), wide(height)) {
        return None;
    }
    let options = unlimited.jpeg_set_out_colorspace(input).set_strict_mode(true);
    let mut samples = JpegDecoder::new_with_options(ZCursor::new(data), options)
        .decode()
        .ok()?;
    if input == ColorSpace::YCCK {
        samples.as_chunks_mut::<4>().0.iter_mut().for_each(ycck_to_cmyk);
    }
    Some(samples)
}

/// libjpeg's `ycck_cmyk_convert` (jdcolor.c): Y, Cb and Cr to red, green and blue with its 16-bit fixed-point tables,
/// then cyan, magenta and yellow are their complements; K passes through.
fn ycck_to_cmyk(pixel: &mut [u8; 4]) {
    const HALF: i32 = 1 << 15;
    let (luma, cb, cr) = (
        i32::from(pixel[0]),
        i32::from(pixel[1]) - 128,
        i32::from(pixel[2]) - 128,
    );
    let red = luma + ((91_881 * cr + HALF) >> 16);
    let green = luma + ((-22_554 * cb + HALF - 46_802 * cr) >> 16);
    let blue = luma + ((116_130 * cb + HALF) >> 16);
    #[expect(clippy::cast_sign_loss, reason = "clamped to 0..=255")]
    let ink = |v: i32| (255 - v).clamp(0, 255) as u8;
    [pixel[0], pixel[1], pixel[2]] = [ink(red), ink(green), ink(blue)];
}

/// A JPEG 2000 file's samples, when its header gives four channels and the enumerated CMYK space, as Pillow requires.
pub(crate) fn jpx_cmyk(data: &[u8]) -> Option<Vec<u8>> {
    let header = jpx::read(data)?;
    let cmyk = header
        .colours
        .iter()
        .any(|c| c.method == 1 && c.enumerated == Some(ENUMERATED_CMYK));
    if !(header.wrapped && header.channels == Some(4) && cmyk) {
        return None;
    }
    let settings = DecodeSettings {
        strict: true,
        ..DecodeSettings::default()
    };
    let image = Image::new(data, &settings).ok()?;
    if too_large(u64::from(image.width()), u64::from(image.height())) {
        return None;
    }
    let mut context = DecoderContext::default();
    Some(image.decode(&mut context).ok()?.data_u8())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::common::jpx::tests::{colr, file, ihdr};

    /// An 8 by 8 CMYK JPEG from Pillow, with an Adobe marker: K steps down the left half, C40 M90 Y200 K10 on the right.
    pub(crate) const JPEG: [u8; 251] = [
        255, 216, 255, 238, 0, 14, 65, 100, 111, 98, 101, 0, 100, 0, 0, 0, 0, 0, 255, 219, 0, 67, 0, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 255, 192, 0, 20, 8, 0, 8, 0, 8, 4, 67, 17, 0,
        77, 17, 0, 89, 17, 0, 75, 17, 0, 255, 196, 0, 21, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 8,
        255, 196, 0, 46, 16, 0, 0, 1, 6, 11, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 18, 19, 20, 22, 23, 2, 8, 10, 21,
        24, 25, 37, 65, 68, 70, 71, 72, 97, 98, 134, 135, 136, 197, 199, 200, 255, 218, 0, 14, 4, 67, 0, 77, 0, 89, 0,
        75, 0, 0, 63, 0, 107, 17, 145, 201, 156, 197, 224, 130, 144, 42, 54, 134, 117, 55, 215, 194, 216, 149, 171,
        176, 7, 117, 95, 55, 132, 32, 72, 210, 146, 86, 17, 72, 43, 126, 124, 116, 78, 163, 18, 176, 170, 223, 60, 174,
        40, 70, 39, 98, 183, 28, 134, 243, 71, 255, 217,
    ];
    /// The same image in JPEG 2000, from Pillow.
    pub(crate) const JP2: [u8; 374] = [
        0, 0, 0, 12, 106, 80, 32, 32, 13, 10, 135, 10, 0, 0, 0, 20, 102, 116, 121, 112, 106, 112, 50, 32, 0, 0, 0, 0,
        106, 112, 50, 32, 0, 0, 0, 45, 106, 112, 50, 104, 0, 0, 0, 22, 105, 104, 100, 114, 0, 0, 0, 8, 0, 0, 0, 8, 0,
        4, 7, 7, 0, 0, 0, 0, 0, 15, 99, 111, 108, 114, 1, 0, 0, 0, 0, 0, 12, 0, 0, 1, 41, 106, 112, 50, 99, 255, 79,
        255, 81, 0, 50, 0, 0, 0, 0, 0, 8, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 4, 7, 1, 1, 7, 1, 1, 7, 1, 1, 7, 1, 1, 255, 82, 0, 12, 0, 0, 0, 1, 0, 3, 4, 4, 0, 1, 255, 92, 0, 13,
        64, 64, 72, 72, 80, 72, 72, 80, 72, 72, 80, 255, 100, 0, 37, 0, 1, 67, 114, 101, 97, 116, 101, 100, 32, 98,
        121, 32, 79, 112, 101, 110, 74, 80, 69, 71, 32, 118, 101, 114, 115, 105, 111, 110, 32, 50, 46, 53, 46, 52, 255,
        144, 0, 10, 0, 0, 0, 0, 0, 165, 0, 1, 255, 147, 207, 180, 8, 9, 151, 207, 180, 8, 8, 63, 207, 180, 8, 6, 223,
        207, 180, 8, 7, 39, 195, 234, 2, 0, 2, 223, 199, 218, 4, 0, 3, 95, 207, 192, 8, 1, 159, 199, 218, 5, 31, 104,
        20, 126, 0, 64, 7, 59, 1, 127, 6, 199, 193, 243, 132, 0, 9, 64, 93, 63, 195, 234, 5, 0, 9, 21, 90, 66, 223,
        199, 218, 10, 0, 9, 21, 41, 196, 223, 199, 218, 11, 31, 104, 44, 31, 80, 32, 11, 146, 148, 235, 187, 11, 146,
        231, 255, 127, 11, 122, 192, 175, 193, 243, 132, 0, 23, 44, 149, 165, 195, 234, 6, 0, 23, 44, 153, 210, 255,
        127, 199, 218, 10, 0, 23, 43, 206, 178, 7, 199, 218, 13, 15, 168, 22, 7, 206, 12, 25, 229, 45, 200, 240, 115,
        3, 88, 249, 84, 95, 25, 229, 63, 255, 217,
    ];

    fn pixels(samples: &[u8]) -> BTreeSet<[u8; 4]> {
        samples.as_chunks::<4>().0.iter().copied().collect()
    }

    #[test]
    fn jpeg() {
        // What Pillow gives (with its Adobe inversion undone).
        let mut want: BTreeSet<[u8; 4]> = [30, 31, 63, 95, 127, 159, 191, 223, 255]
            .map(|k| [255, 255, 255, k])
            .into();
        want.insert([215, 165, 55, 245]);
        assert_eq!(jpeg_cmyk(&JPEG).map(|s| pixels(&s)), Some(want));
        // Marked YCCK, the same data converts as libjpeg converts it.
        let mut ycck = JPEG;
        ycck[17] = 2;
        let mut want: BTreeSet<[u8; 4]> = [30, 31, 63, 95, 127, 159, 191, 223, 255].map(|k| [0, 134, 0, k]).into();
        want.insert([142, 1, 0, 245]);
        assert_eq!(jpeg_cmyk(&ycck).map(|s| pixels(&s)), Some(want));
        assert_eq!(jpeg_cmyk(b"not a JPEG"), None);
        let (head, _) = JPEG.split_at(100);
        assert_eq!(jpeg_cmyk(head), None, "cut short");
        let mut huge = JPEG;
        let sof = huge.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        huge[sof + 5..sof + 9].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(jpeg_cmyk(&huge), None, "too many pixels");
        let mut marked = JPEG;
        marked[JPEG.len() - 10..JPEG.len() - 8].copy_from_slice(&[0xFF, 0x02]);
        assert_eq!(jpeg_cmyk(&marked), None, "an unknown marker in its scan");
        assert!(too_large(20_000, 10_000) && !too_large(10_000, 10_000));
    }

    #[test]
    fn jpeg_2000() {
        let mut want: BTreeSet<[u8; 4]> = [0, 32, 64, 96, 128, 160, 192, 224].map(|k| [0, 0, 0, k]).into();
        want.insert([40, 90, 200, 10]);
        assert_eq!(jpx_cmyk(&JP2).map(|s| pixels(&s)), Some(want));
        assert_eq!(jpx_cmyk(b"not JPEG 2000"), None);
        assert_eq!(jpx_cmyk(&file(&[ihdr(4, 8), colr(16)], &[8; 4])), None, "sRGB");
        assert_eq!(
            jpx_cmyk(&file(&[ihdr(3, 8), colr(12)], &[8; 3])),
            None,
            "three channels"
        );
        assert_eq!(
            jpx_cmyk(&file(&[ihdr(4, 8), colr(12)], &[8; 4])),
            None,
            "no tiles to decode"
        );
        let mut huge = JP2;
        let siz = huge.windows(2).position(|w| w == [0xFF, 0x51]).unwrap();
        huge[siz + 6..siz + 14].copy_from_slice(&[0, 0, 0x4E, 0x20, 0, 0, 0x4E, 0x20]);
        assert_eq!(jpx_cmyk(&huge), None, "too many pixels");
        let mut damaged = JP2;
        damaged[226] = 0x55;
        assert_eq!(jpx_cmyk(&damaged), None, "a damaged tile");
        let (head, _) = JP2.split_at(JP2.len() - 40);
        assert_eq!(jpx_cmyk(head), None, "cut short");
    }
}
