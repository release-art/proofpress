//! Colours to CIE L*a*b* through ICC profiles, as the Python contrast check measured them through Pillow: 8-bit device
//! values (each component rounded to the nearest of 256 steps) through `LittleCMS`, relative colorimetric, into an 8-bit
//! Lab image, so L* moves in steps of 100/255 and a* and b* in whole units.

use lcms2::{CIExyY, ColorSpaceSignature, Intent, PixelFormat, Profile, Transform};

pub(crate) type Lab = [f64; 3];

/// The device families a colour is converted from.
pub(crate) struct Model {
    rgb: Transform<[u8; 3], [u8; 3]>,
    cmyk: Option<Transform<[u8; 4], [u8; 3]>>,
}

/// The D50 white of `LittleCMS`'s `cmsD50_xyY`, which Pillow's Lab profile is made with.
fn d50() -> CIExyY {
    let (x, y, z) = (0.9642, 1.0, 0.8249);
    let sum = x + y + z;
    CIExyY {
        x: x / sum,
        y: y / sum,
        Y: y,
    }
}

impl Model {
    /// A model with `cmyk` for CMYK and `rgb` (else sRGB) for RGB; `None` when a profile does not load or convert.
    pub(crate) fn new(cmyk: Option<&[u8]>, rgb: Option<&[u8]>) -> Option<Self> {
        let srgb = match rgb {
            Some(data) => Profile::new_icc(data),
            None => Ok(Profile::new_srgb()),
        };
        let (lab, srgb) = Profile::new_lab2_context(lcms2::GlobalContext::new(), &d50())
            .ok()
            .zip(srgb.ok())?;
        let intent = Intent::RelativeColorimetric;
        let rgb = Transform::new(&srgb, PixelFormat::RGB_8, &lab, PixelFormat::Lab_8, intent).ok()?;
        let cmyk = match cmyk {
            Some(data) => {
                let profile = Profile::new_icc(data).ok()?;
                Some(Transform::new(&profile, PixelFormat::CMYK_8, &lab, PixelFormat::Lab_8, intent).ok()?)
            }
            None => None,
        };
        Some(Self { rgb, cmyk })
    }

    /// A model from the first CMYK and the first RGB of `profiles` (by the data colour space `LittleCMS` reads), skipping
    /// any it cannot read; `None` when a chosen profile does not convert.
    pub(crate) fn from_profiles(profiles: &[Vec<u8>]) -> Option<Self> {
        let first = |wanted: ColorSpaceSignature| {
            profiles
                .iter()
                .find(|data| Profile::new_icc(data).is_ok_and(|p| p.color_space() == wanted))
                .map(Vec::as_slice)
        };
        Self::new(
            first(ColorSpaceSignature::CmykData),
            first(ColorSpaceSignature::RgbData),
        )
    }

    /// Whether CMYK can be converted.
    pub(crate) fn has_cmyk(&self) -> bool {
        self.cmyk.is_some()
    }

    /// RGB components (0 to 1) as L*a*b*.
    pub(crate) fn rgb(&self, components: [f64; 3]) -> Lab {
        let mut out = [[0u8; 3]];
        self.rgb.transform_pixels(&[components.map(byte)], &mut out);
        decoded(out[0])
    }

    /// CMYK components (0 to 1) as L*a*b*, when there is a CMYK profile.
    pub(crate) fn cmyk(&self, components: [f64; 4]) -> Option<Lab> {
        let mut out = [[0u8; 3]];
        self.cmyk.as_ref()?.transform_pixels(&[components.map(byte)], &mut out);
        Some(decoded(out[0]))
    }
}

/// A component as Pillow takes it: times 255, rounded half to even, clamped.
fn byte(c: f64) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255"
    )]
    let b = (c * 255.0).round_ties_even().clamp(0.0, 255.0) as u8;
    b
}

/// An 8-bit Lab pixel as Pillow's LAB mode stores it: L* scaled to 0..=255, a* and b* offset by 128.
fn decoded([l, a, b]: [u8; 3]) -> Lab {
    [f64::from(l) * 100.0 / 255.0, f64::from(a) - 128.0, f64::from(b) - 128.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMYK_PROFILE: &[u8] = include_bytes!("../../../tests/fixtures/icc/little-cms-test1.icc");

    /// L* as the 8-bit step it is, and a* and b*.
    fn steps([l, a, b]: Lab) -> [f64; 3] {
        [(l * 255.0 / 100.0).round(), a, b]
    }

    #[test]
    fn colours_convert_as_pillow_converts_them() {
        // Pillow's values for the same colours through the same profiles.
        let model = Model::new(Some(CMYK_PROFILE), None).unwrap();
        assert_eq!(steps(model.rgb([0.0, 0.0, 0.2])), [5.0, 11.0, -30.0]);
        assert_eq!(steps(model.rgb([0.6, 0.4, 0.2])), [122.0, 17.0, 37.0]);
        assert_eq!(model.cmyk([0.0; 4]).map(steps), Some([255.0, 0.0, 0.0]));
        assert_eq!(model.cmyk([0.2, 0.4, 0.6, 0.8]).map(steps), Some([74.0, 5.0, 10.0]));
        assert_eq!(
            model.rgb([1.2, -1.0, 0.501]),
            model.rgb([1.0, 0.0, 0.502]),
            "clamped and rounded"
        );
        assert!(model.has_cmyk());
        let srgb_only = Model::new(None, None).unwrap();
        assert_eq!(srgb_only.cmyk([0.0; 4]), None);
        assert!(!srgb_only.has_cmyk());
        let rgb_profile = Model::new(None, Some(CMYK_PROFILE));
        assert!(rgb_profile.is_none(), "a CMYK profile cannot convert RGB");
        assert!(Model::new(Some(b"not a profile"), None).is_none());
        assert!(Model::new(None, Some(b"not a profile")).is_none());
        // A CMYK printer profile header with no tags: LittleCMS opens it, but it converts nothing.
        let mut empty = vec![0u8; 132];
        empty[0..4].copy_from_slice(&132u32.to_be_bytes());
        empty[8] = 4;
        empty[12..16].copy_from_slice(b"prtr");
        empty[16..20].copy_from_slice(b"CMYK");
        empty[20..24].copy_from_slice(b"Lab ");
        empty[36..40].copy_from_slice(b"acsp");
        assert!(Profile::new_icc(&empty).is_ok());
        assert!(Model::new(Some(&empty), None).is_none());
        let chosen = Model::from_profiles(&[b"junk".to_vec(), CMYK_PROFILE.to_vec(), b"more".to_vec()]).unwrap();
        assert_eq!(chosen.cmyk([0.0; 4]).map(steps), Some([255.0, 0.0, 0.0]));
        assert!(!Model::from_profiles(&[]).unwrap().has_cmyk());
    }
}
