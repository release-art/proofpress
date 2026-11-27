//! Tier 8: the checks, each a first-order listener chosen per profile. What each check is, its id, title and clauses,
//! is data in `rust/checks.toml`, published as constants in [`crate::findings`].

use std::panic::{AssertUnwindSafe, catch_unwind};

mod annots;
mod boxes;
mod codecs;
mod colour;
mod complete;
pub(crate) mod contents;
pub(crate) mod contrast;
mod fonts;
mod gray;
mod info;
mod ink;
mod intent;
pub mod inventory;
mod jpx;
mod keys;
mod limits;
mod metadata;
mod modes;
mod names;
mod optional;
mod page;
pub(crate) mod painted;
mod resolution;
mod spots;
mod targets;
mod text;
mod transparency;
mod walk;
mod x4;
mod x4p;
mod x5g;

use crate::document::Document;
use crate::findings::{
    self as f, Check, CheckResult, Clause, FILE_ENCRYPTED, Finding, INTERNAL, Level, Outcome, Report, Severity, Status,
    Structure, X4_ENCRYPT_ABSENT, X4_FILTER_NO_CRYPT, X4_FILTER_NO_LZW, X4_FILTER_STANDARD_ONLY, X4_LIMITS_OBJECTS,
    X4_NAMES_HEX_ESCAPED, X4_NAMES_UTF8, X4_PDF_REFERENCE_CONFORMS, X4_PERMS_UR_ONLY,
};
use crate::pipeline::Limits;
use std::path::{Path, PathBuf};

/// A check's code: what it finds in an open document with the settings it runs with.
type Run = fn(&Document, &Settings) -> Vec<Finding>;

/// A check's code when it may be unable to judge: what it finds, or why it could not tell (a file it needs is not
/// there).
type Judge = fn(&Document, &Settings) -> Result<Vec<Finding>, String>;

/// A check's code over a PDF/X-5 file set: what it finds in the document and its targets, or why it could not tell.
type SetJudge = fn(&Document, &Settings, &targets::Set) -> Result<Vec<Finding>, String>;

/// What the checks run with: the level a file is verified at, and the parameters the print checks take from the
/// profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The level the file is verified at.
    pub level: Level,
    /// The print checks' parameters.
    pub params: Params,
    /// Where the files the document refers to are looked for.
    pub files: Files,
}

impl From<Level> for Settings {
    fn from(level: Level) -> Self {
        Self {
            level,
            params: Params::default(),
            files: Files::Alone,
        }
    }
}

/// Whether a file is verified alone or with the files it refers to (ISO 15930-7 A.1): a PDF/X-4p file's external
/// profile. Nothing is fetched over a network.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Files {
    /// The file alone: a check that needs another file is not evaluated.
    #[default]
    Alone,
    /// The file set: relative URLs resolve against this folder, the file's own, and a referenced file must be there.
    Set(PathBuf),
}

/// The print checks' parameters, each defaulting to today's profile's value.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Params {
    /// `rich-black`: the largest text, in points, that may not be set in a built black.
    pub small_text_pt: f64,
    /// `geometry`, `bleed` and `centring`: the finished page the job expects; none turns them off.
    pub geometry: Option<Geometry>,
    /// `page-size` and `page-rotate`: whether every page must be the first page's size, unrotated.
    pub uniform_pages: bool,
    /// `page-count`: the number the page count must be a multiple of; none or 0 turns it off.
    pub pages_multiple_of: Option<i64>,
    /// `output-condition`: the printing condition an output intent must name, matched without regard to case.
    pub output_condition: Option<String>,
    /// `spot-ink`: the spot inks a job may use, by exact name; none turns the check off.
    pub allow_spots: Option<Vec<String>>,
    /// `colour-space`: whether RGB vector artwork in a CMYK job fails, rather than being advisory.
    pub strict_cmyk: bool,
    /// `icc-profile`: the job's colour model, which decides what its embedded profiles must agree with.
    pub colour_mode: ColourMode,
    /// `gray-only`: whether `DeviceCMYK` on the K plate alone counts as gray.
    pub allow_k_only: bool,
    /// `image-dpi`: the resolution images must reach.
    pub min_dpi: f64,
    /// `image-dpi`: the resolution below which an image is worth a warning, above the minimum.
    pub warn_dpi: Option<f64>,
    /// `image-dpi-max`: the resolution above which an image only grows the file.
    pub max_dpi: Option<f64>,
    /// `image-stretch`: how much more one axis may be scaled than the other, in percent.
    pub max_stretch_pct: Option<f64>,
    /// `text-size`: the smallest text allowed, in points.
    pub min_text_pt: Option<f64>,
    /// `hairline`: the thinnest stroke allowed, in points.
    pub min_stroke_pt: Option<f64>,
    /// `safe-area`: how far inside the trim text must stay, in millimetres.
    pub safe_mm: Option<f64>,
    /// `contrast`: the smallest L* difference text may have from its background; none turns the check off.
    pub min_delta_l: Option<f64>,
    /// `contrast`: the ICC profile to measure through, a file path, instead of the output intents' profiles.
    pub proof_profile: Option<String>,
    /// `x5.g-target-version-policy` (ISO 15930-8 8.4, X5-Q5): whether a target that is another version of the
    /// document a reference names is accepted; with no choice made, such a target is not evaluated.
    pub accept_target_versions: Option<bool>,
}

/// The colour model a job is held to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColourMode {
    /// Printed in four process colours.
    #[default]
    Cmyk,
    /// Shown or printed from RGB.
    Rgb,
    /// Printed in one black ink.
    Gray,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            small_text_pt: 12.0,
            geometry: None,
            uniform_pages: false,
            pages_multiple_of: None,
            output_condition: None,
            allow_spots: None,
            strict_cmyk: false,
            colour_mode: ColourMode::Cmyk,
            allow_k_only: false,
            min_dpi: 300.0,
            warn_dpi: None,
            max_dpi: None,
            max_stretch_pct: None,
            min_text_pt: None,
            min_stroke_pt: None,
            safe_mm: None,
            min_delta_l: None,
            proof_profile: None,
            accept_target_versions: None,
        }
    }
}

/// A finished page: its trim size and bleed in millimetres, the tolerance either is held to, and a different bleed on
/// the binding edge (left on odd pages when `binding` is `left`, right otherwise).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    /// The trim's width.
    pub trim_w_mm: f64,
    /// The trim's height.
    pub trim_h_mm: f64,
    /// The bleed on every side but the binding edge.
    pub bleed_mm: f64,
    /// How far a size or bleed may be off.
    #[serde(default = "Geometry::default_tolerance")]
    pub tolerance_mm: f64,
    /// The bleed on the binding edge, when it differs.
    #[serde(default)]
    pub binding_bleed_mm: Option<f64>,
    /// Which edge odd pages bind on.
    #[serde(default = "Geometry::default_binding")]
    pub binding: String,
}

impl Geometry {
    fn default_tolerance() -> f64 {
        0.25
    }

    fn default_binding() -> String {
        "left".to_owned()
    }
}

/// Every implemented check that reads an open document, with its code. A test holds this list and the
/// `implemented` entries of `rust/checks.toml` equal, apart from the checks judged from tiers 1 to 5's own findings.
/// Every implemented check over a PDF/X-5 file set, with its code.
const SET_JUDGES: &[(&Check, SetJudge)] = &[
    (f::X5_G_TARGET_CONFORMANCE_LEVEL, targets::conformance_level),
    (f::X5_G_TARGET_CONFORMS, targets::conforms),
    (f::X5_G_NO_CIRCULAR_REFS, targets::no_circular),
    (f::X5_G_TARGET_XMP_IDS, targets::xmp_ids),
    (f::X5_G_TARGET_IDENTITY, targets::target_identity),
    (f::X5_G_TARGET_VERSION_POLICY, targets::version_policy),
    (f::X5_G_TARGET_PAGE_EXISTS, targets::page_exists),
    (f::X5_G_SAME_PRINTING_CONDITION, targets::same_printing_condition),
    (f::X5_G_TARGET_NO_OCG, targets::no_ocg),
];

/// Every implemented check whose code may be unable to judge, with its code; a test counts these with [`RUNS`].
const JUDGES: &[(&Check, Judge)] = &[
    (f::X4_ICC_CMYK_NOT_INTENT_PROFILE, colour::not_intent_profile_at_level),
    (f::X4P_FILESET_CHECKSUM_MATCHES, x4p::checksum_matches),
    (f::X4P_FILESET_FIELDS_MATCH, x4p::fields_match),
    (f::X4P_FILESET_PROFILE_VALID, x4p::profile_valid),
    (f::X5_N_PROFILE_ISO15076, x4p::x5n_profile_iso15076),
    (
        f::X5_N_COLORANT_TABLE_MATCHES_PROFILE,
        x4p::x5n_colorant_table_matches_profile,
    ),
    (f::X1A_IDENTITY_CONFORMANCE, info::conformance),
];

const RUNS: &[(&Check, Run)] = &[
    (
        f::X13_OUTPUT_INTENT_ICC_NAME_NEEDS_REGISTRY,
        intent::icc_name_needs_registry,
    ),
    (
        f::X13_OUTPUT_INTENT_DEST_PROFILE_BY_REGISTRY,
        intent::dest_profile_by_registry,
    ),
    (
        f::X13_OUTPUT_INTENT_DEST_PROFILE_BY_ICC_NAME,
        intent::dest_profile_by_icc_name,
    ),
    (
        f::X3_OUTPUT_INTENT_DEST_PROFILE_FOR_COLOUR,
        colour::x3_dest_profile_for_colour,
    ),
    (f::X1A_OUTPUT_INTENT_PROFILE_ATOB1, intent::profile_atob1),
    (f::X1A_OUTPUT_INTENT_CMYK, intent::cmyk),
    (
        f::X1A_OUTPUT_INTENT_OUTPUT_CONDITION_UNREGISTERED,
        intent::output_condition_unregistered,
    ),
    (f::X3_OUTPUT_INTENT_INFO, intent::info),
    (f::X13_OUTPUT_INTENT_PROFILE_VERSION, intent::older_profile_version),
    (f::X3_ICC_PROFILE_VERSION, colour::x3_profile_version),
    (
        f::X3_OUTPUT_INTENT_REGISTRY_NAME_COLOR_ORG_2003,
        intent::registry_name_color_org_2003,
    ),
    (f::X1A_COLOUR_PRINT_SPACES, colour::x1a_print_spaces),
    (f::X1A_COLOUR_NO_DEFAULT_SPACE, colour::x1a_unused_default),
    (f::X1A_SPOT_ALTERNATE_CMYK_OR_GRAY, colour::x1a_alternates),
    (f::X13_TRANSPARENCY_SMASK, transparency::smask),
    (f::X13_TRANSPARENCY_EXTGSTATE_VALUES, transparency::extgstate_values),
    (f::X13_TRANSPARENCY_NO_GROUP, transparency::no_group),
    (f::X13_TRAPPED_PRESENT, info::trapped_present),
    (f::X13_TRAPPED_TRUE_OR_FALSE, info::trapped_true_or_false),
    (f::X13_TRAPPED_TRAPNET_IMPLIES_TRUE, annots::x13_trapnet_implies_true),
    (f::X1A_TRAPNET_PCM_CMYK, annots::x1a_pcm),
    (f::X13_IDENTITY_VERSION_PRESENT, info::version_present),
    (f::X13_IDENTITY_VERSION_VALUE, info::version_value),
    (f::X13_INFO_CREATION_DATE, info::creation_date),
    (f::X13_INFO_MOD_DATE, info::mod_date),
    (f::X13_INFO_TITLE, info::title),
    (f::X13_INFO_AGENT_NAMES, info::agent_names),
    (f::X13_XMP_MOD_DATE_AGREES, info::mod_date_agrees),
    (X4_FILTER_NO_LZW, |d, _| x4::no_lzw(d)),
    (X4_FILTER_NO_CRYPT, |d, _| x4::no_crypt(d)),
    (X4_FILTER_STANDARD_ONLY, |d, _| x4::standard_only(d)),
    (X4_PERMS_UR_ONLY, |d, _| x4::ur_only(d)),
    (X4_LIMITS_OBJECTS, |d, _| x4::object_count(d)),
    (f::X4_COMPLETE_RESOURCES_INSIDE, complete::resources_inside),
    (f::X4_LIMITS_LEXICAL, limits::lexical),
    (f::X4_LIMITS_GRAPHICS, limits::graphics),
    (X4_NAMES_UTF8, |d, _| x4::names_utf8(d)),
    (X4_NAMES_HEX_ESCAPED, |d, _| x4::names_escaped(d)),
    (f::X4_NAMES_OTHER_RECOMMENDED, names::other_recommended),
    (f::X4_TRAPPED_PRESENT, metadata::trapped_present),
    (f::X4_TRAPPED_TRUE_OR_FALSE, metadata::trapped_true_or_false),
    (f::X4_XMP_PRESENT, metadata::present),
    (f::X4_XMP_NAMESPACE_PREFIXES, metadata::prefixes),
    (f::X4_XMP_UNFILTERED, metadata::unfiltered),
    (f::X4_INFO_XMP_ANALOGUE_PRESENT, metadata::analogue_present),
    (f::X4_INFO_XMP_ANALOGUE_EQUAL, metadata::analogue_equal),
    (f::X4_INFO_PDFX_VERSION_PDFDOC, metadata::info_version_pdfdoc),
    (f::X4_INFO_AUTHOR_SINGLE_CREATOR, metadata::author_single_creator),
    (f::X4_XMP_NO_BYTES_ENCODING_ATTRS, metadata::header_attributes),
    (f::X4_XMP_DOCUMENT_ID, metadata::document_id),
    (f::X4_XMP_VERSION_ID, metadata::version_id),
    (f::X4_XMP_RENDITION_CLASS, metadata::rendition_class),
    (f::X4_XMP_DOCUMENT_ID_UUID, metadata::document_id_uuid),
    (f::X4_XMP_CREATE_DATE, metadata::create_date),
    (f::X4_XMP_MODIFY_DATE, metadata::modify_date),
    (f::X4_XMP_METADATA_DATE, metadata::metadata_date),
    (f::X4_XMP_TITLE, metadata::title),
    (f::X4_XMP_AGENT_NAMES, metadata::agent_names),
    (f::X4_TRAILER_ID, metadata::trailer_id),
    (f::X4_XMP_WELL_FORMED, metadata::well_formed),
    (f::X4_IDENTITY_VERSION_PRESENT, metadata::version_present),
    (f::X4_IDENTITY_VERSION_VALUE, metadata::version_value),
    (f::X4_IDENTITY_PDFXID_ONLY_VERSION, metadata::pdfxid_only_version),
    (f::X4_IDENTITY_NO_INFO_CONFORMANCE, metadata::no_info_conformance),
    (f::X4_XMP_CONFORMS, metadata::conforms),
    (
        f::X4_INFO_UNLISTED_NOT_IN_PREDEFINED_SCHEMA,
        metadata::unlisted_not_predefined,
    ),
    (f::X4_OUTPUT_INTENT_PRESENT, intent::present),
    (f::X4_OUTPUT_INTENT_UNIQUE, intent::unique),
    (f::X4_OUTPUT_INTENT_IDENTIFIER_PRESENT, intent::identifier_present),
    (
        f::X4_OUTPUT_INTENT_IDENTIFIER_TEXT_STRING,
        intent::identifier_text_string,
    ),
    (f::X4_OUTPUT_INTENT_REGISTRY_NAME_URL, intent::registry_name_url),
    (f::X4_OUTPUT_INTENT_DEST_PROFILE_PRESENT, intent::dest_profile_present),
    (f::X4_OUTPUT_INTENT_NO_DEST_PROFILE_REF, intent::no_dest_profile_ref),
    (f::X4_OUTPUT_INTENT_OUTPUT_CONDITION, intent::output_condition),
    (f::X4_OUTPUT_INTENT_CHANNELS, intent::channels),
    (f::X4_OUTPUT_INTENT_PROFILE_CLASS_PRTR, intent::profile_class),
    (f::X4_OUTPUT_INTENT_PROFILE_VERSION, intent::profile_version),
    (f::X4_OUTPUT_INTENT_IDENTIFIER_REGISTERED, intent::identifier_registered),
    (
        f::X4_OUTPUT_INTENT_REGISTRY_NAME_COLOR_ORG,
        intent::registry_name_color_org,
    ),
    (
        f::X4_OUTPUT_INTENT_IDENTIFIER_IN_REGISTRY,
        intent::identifier_in_registry,
    ),
    (f::X4_BOXES_TRIM_OR_ART, boxes::trim_or_art),
    (f::X4_BOXES_NOT_BOTH, boxes::not_both),
    (f::X4_BOXES_PREFER_TRIM, boxes::prefer_trim),
    (f::X4_BOXES_WITHIN_BLEED, boxes::within_bleed),
    (f::X4_BOXES_WITHIN_CROP, boxes::within_crop),
    (f::X4_BOXES_WITHIN_MEDIA, boxes::within_media),
    (f::X4_XOBJECT_NO_OPI, keys::no_opi),
    (f::X4_XOBJECT_NO_REF, keys::no_ref),
    (f::X4_STREAM_NO_EXTERNAL_F, keys::no_external_f),
    (f::X4_FILESPEC_EMBEDDED, keys::filespec_embedded),
    (f::X4_EXTGSTATE_NO_TR, keys::no_tr),
    (f::X4_EXTGSTATE_NO_HTP, keys::no_htp),
    (f::X4_EXTGSTATE_TR2_DEFAULT, keys::tr2_default),
    (f::X4_HALFTONE_TYPE_1_OR_5, keys::halftone_type),
    (f::X4_HALFTONE_NO_NAME, keys::halftone_name),
    (f::X4_HALFTONE_TRANSFER_FUNCTION, keys::halftone_transfer),
    (f::X4_XOBJECT_NO_POSTSCRIPT, keys::no_postscript),
    (f::X4_XOBJECT_NO_SUBTYPE2_PS, keys::no_subtype2_ps),
    (f::X4_IMAGE_NO_DEFAULT_FOR_PRINTING, keys::no_default_for_printing),
    (f::X4_ACTIONS_ABSENT, keys::no_actions),
    (f::X4_JAVASCRIPT_ABSENT, keys::no_javascript),
    (f::X4_VIEWER_PREFS_AREAS, keys::viewer_areas),
    (f::X4_NAMES_NO_ALTERNATE_PRESENTATIONS, keys::no_alternate_presentations),
    (f::X4_PAGE_NO_PRES_STEPS, keys::no_pres_steps),
    (f::X4_ACROFORM_NO_XFA, keys::no_xfa),
    (f::X4_OC_CONFIG_NAME_PRESENT, optional::name_present),
    (f::X4_OC_CONFIG_NAME_UNIQUE, optional::name_unique),
    (f::X4_OC_ORDER_COMPLETE, optional::order_complete),
    (f::X4_OC_NO_AS, optional::no_as),
    (f::X4_ANNOT_OUTSIDE_BLEED, annots::outside_bleed),
    (f::X4_ANNOT_PRINTERMARK_OUTSIDE_TRIM, annots::printermark_outside_trim),
    (f::X4_TRAPPED_TRAPNET_IMPLIES_TRUE, annots::trapnet_implies_true),
    (f::X4_TRAPNET_FONT_FAUXING_EMPTY, annots::font_fauxing),
    (f::X4_TRAPNET_PCM_MATCHES_INTENT, annots::pcm_matches_intent),
    (f::PAGE_MEDIA_BOX, page::media_box_present),
    (f::X5_IDENTIFICATION_VALUE, metadata::x5_version_value),
    (f::X5_G_REF_PRESENT, x5g::ref_present),
    (f::X5_G_PROXY_IS_FORM, x5g::proxy_is_form),
    (f::X5_G_REF_ID_REQUIRED, x5g::ref_id_required),
    (f::X5_G_REF_FILESPEC_EXTERNAL, keys::filespec_embedded_x5),
    (f::X5_G_PROXY_METADATA, x5g::proxy_metadata),
    (f::X5_G_PROXY_RENDITION_OF, x5g::proxy_rendition_of),
    (f::X5_G_DOCUMENT_ID_UUID, x5g::document_id_uuid),
    (f::X5_N_TRAPNET_PCM, annots::x5n_pcm),
    (f::X5_N_TRAPNET_SEPARATIONS, annots::x5n_separations),
    (f::X5_N_URLS_EF_ALLOWED, x4p::x5n_urls_ef_allowed),
    (f::X5_N_PROFILECS_XCLR, x4p::x5n_profile_cs_xclr),
    (f::X5_N_3CLR_4CLR_NOT_RGB_CMYK, x4p::x5n_3clr_4clr),
    (f::X5_N_COLORANT_TABLE_PRESENT, x4p::x5n_colorant_table_present),
    (f::X5_N_COLORANT_TABLE_COUNT, x4p::x5n_colorant_table_count),
    (f::X5_N_DEVICEGRAY_NEEDS_BLACK, colour::x5n_gray),
    (f::X5_N_DEVICECMYK_NEEDS_CMYK, colour::x5n_cmyk),
    (f::X5_N_NO_DEVICERGB, colour::x5n_rgb),
    (f::X5_N_DEVICEN_SUBTYPE, spots::x5n_devicen_subtype),
    (f::X5_N_DEVICEN_NO_PROCESS, spots::x5n_devicen_no_process),
    (f::X5_N_TRANSPARENCY_GROUP_CS, modes::x5n_group_cs),
    (f::X4P_OUTPUT_INTENT_DEST_PROFILE_REF, x4p::dest_profile_ref),
    (f::X4P_OUTPUT_INTENT_NO_DEST_PROFILE, x4p::no_dest_profile),
    (f::X4P_REF_PROFILE_NAME, x4p::profile_name),
    (f::X4P_REF_URLS, x4p::urls_present),
    (f::X4P_REF_URLS_NO_EF_PLATFORM, x4p::urls_no_ef_platform),
    (f::X4P_REF_CHECKSUM, x4p::checksum),
    (f::X4P_REF_PROFILE_CS, x4p::profile_cs),
    (f::X4P_REF_ICC_VERSION, x4p::icc_version),
    (f::X4P_REF_NO_COLORANT_TABLE, x4p::no_colorant_table),
    (f::CONTENT_DAMAGED, contents::damaged),
    (f::CONTENT_OPERATOR, contents::operator),
    (f::CONTENT_COMPATIBILITY, contents::compatibility),
    (f::X4_CONTENT_KNOWN_OPERATORS, contents::known_operators),
    (f::X4_CONTENT_NO_BX_EX, contents::no_bx_ex),
    (f::X4_INTENT_KNOWN_VALUES, contents::known_intents),
    (f::X4_COLOUR_DEVICE_SPACE_MATCHES_INTENT, colour::device_spaces),
    (f::X4_SPOT_ALTERNATE_SPACE_RESTRICTED, colour::alternate_spaces),
    (f::X4_ICC_PROFILE_VERSION, colour::profile_version),
    (f::X4_ICC_OVERPRINT_MODE_ZERO, colour::overprint_mode),
    (f::X4_TRANSPARENCY_GROUP_CS_RESTRICTED, colour::group_spaces),
    (f::X4_PRE_SEPARATED_ABSENT, colour::pre_separated),
    (f::X4_SPOT_COLORANTS_ENTRY, spots::colorants_entry),
    (f::X4_SPOT_COLORANTS_MATCH_DEVICEN, spots::colorants_match),
    (f::X4_SPOT_PROCESS_COLORSPACE, spots::process_space),
    (f::X4_SPOT_NO_DOTGAIN, spots::no_dot_gain),
    (f::X4_SPOT_CONFUSING_NAMES, spots::confusing_names),
    (f::X4_SPOT_CONSISTENT_DEFINITION, spots::consistent_definitions),
    (f::X4_SPOT_PRINTING_ORDER_CONSISTENT, spots::printing_order),
    (f::X4_SPOT_SOLIDITIES_CONSISTENT, spots::solidities),
    (f::X4_TRANSPARENCY_BLEND_MODES, spots::blend_modes),
    (f::X4_FONT_EMBEDDED, fonts::embedded),
    (f::X4_FONT_GLYPHS_PRESENT, fonts::glyphs_present),
    (f::X4_FONT_NO_NOTDEF, fonts::no_notdef),
    (f::X4_FONT_WIDTHS_CONSISTENT, fonts::widths_consistent),
    (f::X4_FONT_TRUETYPE_NONSYMBOLIC_ENCODING, fonts::nonsymbolic_encoding),
    (
        f::X4_FONT_TRUETYPE_NONSYMBOLIC_NO_DIFFERENCES,
        fonts::nonsymbolic_no_differences,
    ),
    (f::X4_FONT_TRUETYPE_SYMBOLIC_NO_ENCODING, fonts::symbolic_no_encoding),
    (f::X4_FONT_TRUETYPE_SYMBOLIC_ONE_CMAP, fonts::symbolic_one_cmap),
    (f::X4_IMAGE_DCT_VALID, codecs::dct_valid),
    (f::X4_IMAGE_JPX_VALID, jpx::valid),
    (f::X4_IMAGE_JBIG2_VALID, codecs::jbig2_valid),
    (f::X4_JPX_BASELINE, jpx::baseline),
    (f::X4_JPX_CHANNELS, jpx::channels),
    (f::X4_JPX_ONE_COLOUR_SPACE, jpx::one_colour_space),
    (f::X4_JPX_NO_CIEJAB, jpx::no_ciejab),
    (f::X4_JPX_BIT_DEPTH, jpx::bit_depth),
    (f::X4_JPX_UNIFORM_DEPTH, jpx::uniform_depth),
    (f::X4_JPX_CMYK_AS_DEVICE, jpx::cmyk_as_device),
    (f::XOBJECT_MISSING, painted::xobject_missing),
    (f::OUTSIDE_MEDIA, painted::outside_media),
    (f::OVERPRINT, ink::overprint),
    (f::WHITE_OVERPRINT, ink::white_overprint),
    (f::RICH_BLACK, ink::rich_black),
    (f::K_ONLY_TEXT, ink::k_only_text),
    (f::GEOMETRY, page::geometry),
    (f::BLEED, page::bleed),
    (f::CENTRING, page::centring),
    (f::PAGE_COUNT, page::page_count),
    (f::PAGE_SIZE, page::page_size),
    (f::PAGE_ROTATE, page::page_rotate),
    (f::OUTPUT_CONDITION, page::output_condition),
    (f::ARTWORK_BLEED, painted::artwork_bleed),
    (f::OUTSIDE_BLEED, painted::outside_bleed),
    (f::BLEND_SPACE, modes::blend_space),
    (f::SPOT_INK, modes::spot_ink),
    (f::COLOUR_SPACE, modes::colour_space),
    (f::RGB_ONLY, modes::rgb_only),
    (f::ICC_PROFILE, modes::icc_profile),
    (f::SPOT_DEFINITION, modes::spot_definition),
    (f::GRAY_ONLY, gray::gray_only),
    (f::CONTRAST, contrast::contrast),
    (f::IMAGE_DPI, resolution::image_dpi),
    (f::IMAGE_DPI_MAX, resolution::image_dpi_max),
    (f::IMAGE_STRETCH, resolution::image_stretch),
    (f::MISSING_GLYPH, text::missing_glyph),
    (f::FONT_EMBEDDED, text::font_embedded),
    (f::FONT_TYPE3, text::font_type3),
    (f::TEXT_SIZE, text::text_size),
    (f::HAIRLINE, text::hairline),
    (f::SAFE_AREA, text::safe_area),
];

/// Verify `bytes`, the file named `file`, at `level`: open it, reading everything reachable, report what tiers 1 to 5
/// found, and run every implemented check of the level. A check whose scope holds a gap is not evaluated, and on a
/// refused file nothing is. The level's checks still to be written are reported as not evaluated, so a level that is
/// not fully checked yet cannot pass.
#[must_use]
pub fn verify(file: &str, bytes: &[u8], level: Level) -> Report {
    verify_with(file, bytes, level, &Limits::default())
}

/// [`verify`] within other limits than the defaults.
#[must_use]
pub fn verify_with(file: &str, bytes: &[u8], level: Level, limits: &Limits) -> Report {
    verify_in(file, bytes, &level.into(), limits)
}

/// [`verify`] with the print checks' parameters too.
#[must_use]
pub fn verify_settings(file: &str, bytes: &[u8], settings: &Settings) -> Report {
    verify_in(file, bytes, settings, &Limits::default())
}

/// [`verify_settings`] at the PDF/X-1a or PDF/X-3 level the file claims in its identification, when it claims one,
/// and otherwise at `settings`' level. Used when no level is asked for.
#[must_use]
pub fn verify_claimed(file: &str, bytes: &[u8], settings: &Settings) -> Report {
    verify_chain(file, bytes, settings, &Limits::default(), &[], true)
}

/// The checks of `catalogue` at `level` still to be written, each not evaluated, so a level that is not fully checked
/// cannot pass.
fn unwritten(catalogue: &'static [&'static Check], level: Level) -> Vec<CheckResult> {
    let todo = catalogue
        .iter()
        .filter(|c| c.applies.contains(&level) && c.status == Status::Todo);
    todo.map(|id| CheckResult {
        id,
        outcome: Outcome::NotEvaluated("not written yet".into()),
    })
    .collect()
}

/// A check's code, of any kind.
#[derive(Clone, Copy)]
enum Code {
    Run(Run),
    Judge(Judge),
    Set(SetJudge),
}

fn verify_in(file: &str, bytes: &[u8], settings: &Settings, limits: &Limits) -> Report {
    verify_chain(file, bytes, settings, limits, &[], false)
}

/// [`verify_in`] for a file that is a target of the files in `chain`, outermost first (ISO 15930-8 8.2); with
/// `detect`, at the older level the file claims, if any (see [`verify_claimed`]).
fn verify_chain(
    file: &str,
    bytes: &[u8],
    settings: &Settings,
    limits: &Limits,
    outer: &[PathBuf],
    detect: bool,
) -> Report {
    // The files being verified, this one last, to tell a reference that loops back.
    let mut chain = outer.to_vec();
    if let Files::Set(folder) = &settings.files {
        let path = folder.join(Path::new(file).file_name().unwrap_or_default());
        chain.push(std::fs::canonicalize(&path).unwrap_or(path));
    }
    let set = std::cell::OnceCell::new();
    let opened = Document::open(bytes, limits);
    let claimed = match (&opened, detect) {
        (Ok(doc), true) => targets::claimed_older_level(doc),
        _ => None,
    };
    let settings = &Settings {
        level: claimed.unwrap_or(settings.level),
        ..settings.clone()
    };
    let level = settings.level;
    let applies = |check: &Check| check.applies.contains(&level) && check.status == Status::Implemented;
    let runs = RUNS.iter().map(|&(check, run)| (check, Code::Run(run)));
    let judges = JUDGES.iter().map(|&(check, judge)| (check, Code::Judge(judge)));
    let set_judges = SET_JUDGES.iter().map(|&(check, judge)| (check, Code::Set(judge)));
    let enabled = runs.chain(judges).chain(set_judges).filter(|(check, _)| applies(check));
    let unwritten = unwritten(f::ALL, level).into_iter();
    let mut report = match opened {
        Err(refusal) => {
            let unread = enabled.map(|(id, _)| CheckResult {
                id,
                outcome: Outcome::NotEvaluated("the file was refused".into()),
            });
            judged_from_structure(
                file,
                Structure::Refused,
                unread.chain(unwritten).collect(),
                vec![*refusal],
                &applies,
            )
        }
        Ok(doc) => {
            let structure = if doc.damaged() {
                Structure::Damaged
            } else {
                Structure::Sound
            };
            let mut findings = doc.findings();
            let mut checks = Vec::new();
            for (id, code) in enabled {
                let outcome = if structure == Structure::Damaged {
                    Outcome::NotEvaluated("objects it would read could not be read".into())
                } else {
                    let unjudged = std::cell::RefCell::new(None);
                    let or_note = |result: Result<Vec<Finding>, String>| {
                        result.unwrap_or_else(|why| {
                            unjudged.replace(Some(why));
                            Vec::new()
                        })
                    };
                    let found = guarded(id, &|| match code {
                        Code::Run(run) => run(&doc, settings),
                        Code::Judge(judge) => or_note(judge(&doc, settings)),
                        Code::Set(judge) => {
                            let set = set.get_or_init(|| targets::resolve(&doc, settings, limits, &chain));
                            or_note(judge(&doc, settings, set))
                        }
                    });
                    let outcome = match unjudged.into_inner() {
                        Some(why) => Outcome::NotEvaluated(why),
                        None if found.is_empty() => Outcome::Pass,
                        None => Outcome::Fail,
                    };
                    findings.extend(found);
                    outcome
                };
                checks.push(CheckResult { id, outcome });
            }
            checks.extend(unwritten);
            judged_from_structure(file, structure, checks, findings, &applies)
        }
    };
    report.level = level.title();
    for finding in &mut report.findings {
        finding.clause = cited_at(finding.check, finding.clause, level);
    }
    report
}

/// The clause a finding of `check` citing `clause` cites at `level`: at an older level, a reused X-4 check's ISO 15930-7
/// clause becomes that level's own (`rust/checks.toml`'s `cite`).
fn cited_at(check: &Check, clause: Option<&'static Clause>, level: Level) -> Option<&'static Clause> {
    let x4 = clause.filter(|c| c.standard == "iso-15930-7");
    x4.and_then(|c| check.cites.iter().find(|(l, n, _)| *l == level && *n == c.number))
        .map(|(_, _, to)| to)
        .or(clause)
}

/// The report, with the checks judged from tiers 1 to 5's findings added: `x4.encrypt.absent` fails on an encrypted
/// file, and `x4.pdf-reference.conforms` fails on every error those tiers found (6.1.1), naming it.
fn judged_from_structure(
    file: &str,
    structure: Structure,
    mut checks: Vec<CheckResult>,
    mut findings: Vec<Finding>,
    applies: &dyn Fn(&Check) -> bool,
) -> Report {
    let encrypted = findings.iter().any(|f| f.check == FILE_ENCRYPTED);
    if applies(X4_ENCRYPT_ABSENT) {
        let outcome = if encrypted { Outcome::Fail } else { Outcome::Pass };
        if encrypted {
            findings.push(Finding::new(X4_ENCRYPT_ABSENT, "the file is encrypted").citing("6.15"));
        }
        checks.push(CheckResult {
            id: X4_ENCRYPT_ABSENT,
            outcome,
        });
    }
    if applies(X4_PDF_REFERENCE_CONFORMS) {
        let invalid: Vec<Finding> = findings
            .iter()
            .filter(|f| {
                f.severity == Severity::Error && f.check.applies.contains(&Level::Print) && f.check != FILE_ENCRYPTED
            })
            .map(|f| {
                let mut conforms = Finding::new(
                    X4_PDF_REFERENCE_CONFORMS,
                    format!("not valid PDF ({}): {}", f.check.id, f.message),
                )
                .citing("6.1.1")
                .at(f.chain.clone());
                conforms.caused_by = f.event;
                conforms
            })
            .collect();
        let outcome = match (encrypted, invalid.is_empty()) {
            (true, true) => Outcome::NotEvaluated("the file is encrypted, so its objects cannot be read".into()),
            (_, true) => Outcome::Pass,
            (_, false) => Outcome::Fail,
        };
        findings.extend(invalid);
        checks.push(CheckResult {
            id: X4_PDF_REFERENCE_CONFORMS,
            outcome,
        });
    }
    Report::new(file, structure, checks, findings)
}

/// Run one check, turning a panic into an `internal` finding so the other checks still run and nothing panics into the
/// caller (`PyO3` would raise `PanicException`, which `except Exception` does not catch). Unwinding is safe here: a check
/// reads the frozen object table and owns everything it writes.
pub fn guarded(check: &'static Check, run: &dyn Fn() -> Vec<Finding>) -> Vec<Finding> {
    catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|panic| {
        let reason = panic
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        vec![Finding::new(
            INTERNAL,
            format!("check {} failed inside proof-pdf: {reason}", check.id),
        )]
    })
}

/// Documents for the checks' unit tests.
#[cfg(test)]
pub(crate) mod test_doc {
    use crate::document::Document;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    /// A document whose pages have the dictionary entries and content of `pages`, whose catalog has `catalog` besides
    /// `/Pages`, and with the objects `more` (numbered below 100).
    pub(crate) fn paged(pages: &[(&str, &str)], catalog: &str, more: &[(u32, Vec<u8>)]) -> Document {
        let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 100 + 2 * i)).collect();
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (1, format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>").into_bytes()),
            (
                2,
                format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), kids.len()).into_bytes(),
            ),
        ];
        for (i, (entries, data)) in pages.iter().enumerate() {
            let n = u32::try_from(100 + 2 * i).unwrap();
            let page = format!("<< /Type /Page /Parent 2 0 R {entries} /Contents {} 0 R >>", n + 1);
            objects.push((n, page.into_bytes()));
            objects.push((
                n + 1,
                format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes(),
            ));
        }
        objects.extend(more.iter().cloned());
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::{Verdict, X4_TRAPPED_PRESENT};

    /// Every implemented PDF/X check has code: a run in `RUNS`, or a verdict judged from tiers 1 to 5's findings. House
    /// checks (`print`) are raised by the tiers themselves.
    #[test]
    fn every_implemented_check_has_code() {
        let judged = [X4_ENCRYPT_ABSENT, X4_PDF_REFERENCE_CONFORMS];
        for check in crate::findings::ALL.iter().filter(|c| c.status == Status::Implemented) {
            let run = RUNS.iter().any(|(c, _)| c == check)
                || JUDGES.iter().any(|(c, _)| c == check)
                || SET_JUDGES.iter().any(|(c, _)| c == check);
            let (judged, tiers) = (judged.contains(check), check.applies.contains(&Level::Print));
            assert!(
                run | judged | tiers,
                "{} is implemented in checks.toml but has no code",
                check.id
            );
        }
        let set = SET_JUDGES.iter().map(|(c, _)| c);
        for check in RUNS
            .iter()
            .map(|(c, _)| c)
            .chain(JUDGES.iter().map(|(c, _)| c))
            .chain(set)
        {
            assert_eq!(
                check.status,
                Status::Implemented,
                "{} has code but checks.toml says otherwise",
                check.id
            );
        }
    }

    #[test]
    fn valid_pdf_is_judged_from_the_tiers_errors() {
        let all = |_: &Check| true;
        let outcome = |findings: Vec<Finding>| {
            let report = judged_from_structure("x.pdf", Structure::Sound, Vec::new(), findings, &all);
            let conforms = report
                .checks
                .into_iter()
                .find(|c| c.id == X4_PDF_REFERENCE_CONFORMS)
                .unwrap();
            let raised = report
                .findings
                .iter()
                .filter(|f| f.check == X4_PDF_REFERENCE_CONFORMS)
                .count();
            (conforms.outcome, raised)
        };
        let damaged = Finding::new(crate::findings::OBJECT_DAMAGED, "object 2 cannot be read");
        assert_eq!(outcome(vec![damaged]), (Outcome::Fail, 1));
        let warning = Finding::new(crate::findings::SYNTAX_DUPLICATE_KEY, "a key twice");
        assert_eq!(outcome(vec![warning]), (Outcome::Pass, 0));
        let encrypted = Finding::new(FILE_ENCRYPTED, "encrypted");
        let why = "the file is encrypted, so its objects cannot be read".to_owned();
        assert_eq!(outcome(vec![encrypted]), (Outcome::NotEvaluated(why), 0));
    }

    #[test]
    fn a_failing_check_fails_the_file() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/pdfs/x4-lzw.pdf"
        ))
        .unwrap();
        let report = verify("x4-lzw.pdf", &bytes, Level::X4);
        let lzw = report.checks.iter().find(|c| c.id == X4_FILTER_NO_LZW).unwrap();
        assert_eq!((report.verdict, &lzw.outcome), (Verdict::Fail, &Outcome::Fail));
    }

    #[test]
    fn a_reused_check_cites_the_older_levels_own_clause() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/pdfs/x4-lzw.pdf"
        ))
        .unwrap();
        let cited = |level: Level| {
            let report = verify("x4-lzw.pdf", &bytes, level);
            let lzw = report.findings.iter().find(|f| f.check == X4_FILTER_NO_LZW).unwrap();
            lzw.clause.map(|c| (c.standard, c.number))
        };
        assert_eq!(cited(Level::X4), Some(("iso-15930-7", "6.8")));
        assert_eq!(cited(Level::X32002), Some(("iso-15930-3", "6.4")));
        assert_eq!(cited(Level::X32003), Some(("iso-15930-6", "6.5")));
        // A clause outside ISO 15930-7, or none, is left alone.
        let other = f::FILE_HEADER.clauses.first();
        assert_eq!(cited_at(f::FILE_HEADER, other, Level::X32003), other);
        assert_eq!(cited_at(X4_FILTER_NO_LZW, None, Level::X32003), None);
    }

    #[test]
    fn a_check_still_to_be_written_is_not_evaluated() {
        static TODO: Check = Check {
            cites: &[],
            id: "x5.example",
            aliases: &[],
            title: "An example",
            clauses: &[],
            severity: Severity::Error,
            applies: &[Level::X5n],
            status: Status::Todo,
            tool: None,
        };
        static CATALOGUE: [&Check; 2] = [&TODO, X4_FILTER_NO_LZW];
        let unjudged = unwritten(&CATALOGUE, Level::X5n);
        assert_eq!(unjudged.len(), 1);
        assert_eq!(
            (unjudged[0].id.id, &unjudged[0].outcome),
            ("x5.example", &Outcome::NotEvaluated("not written yet".into()))
        );
        assert_eq!(unwritten(&CATALOGUE, Level::X4), []);
    }

    #[test]
    fn a_file_claiming_an_older_level_is_verified_at_it_when_none_is_asked_for() {
        use crate::document::test_pdf::classic;
        let file = |info: &str| {
            classic(
                &[
                    (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
                    (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
                    (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>"),
                    (4, info.as_bytes()),
                ],
                "<< /Size SIZE /Root 1 0 R /Info 4 0 R >>",
            )
        };
        let x3 = file("<< /GTS_PDFXVersion (PDF/X-3:2002) >>");
        let report = verify_claimed("x3.pdf", &x3, &Level::X4.into());
        assert_eq!(report.level, Some("PDF/X-3:2002"));
        assert!(
            report.checks.iter().all(|c| c.id.applies.contains(&Level::X32002)),
            "{report:?}"
        );
        assert_eq!(report.verdict, Verdict::Fail, "its checks not yet written fail it");
        // Asked for a level, or claiming none of the older ones: the level given.
        assert_eq!(verify_settings("x3.pdf", &x3, &Level::X4.into()).level, Some("PDF/X-4"));
        let plain = file("<< >>");
        assert_eq!(
            verify_claimed("plain.pdf", &plain, &Level::X4.into()).level,
            Some("PDF/X-4")
        );
        assert_eq!(
            verify_claimed("bad.pdf", b"%PDF-1.3", &Level::X4.into()).level,
            Some("PDF/X-4")
        );
        assert_eq!(verify_settings("x3.pdf", &x3, &Level::Print.into()).level, None);
    }

    #[test]
    fn a_level_runs_only_its_own_checks() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/pdfs/x4-good.pdf"
        ))
        .unwrap();
        let x5n = verify("x4-good.pdf", &bytes, Level::X5n);
        assert!(x5n.checks.iter().all(|c| c.id.applies.contains(&Level::X5n)), "{x5n:?}");
        let todo = f::ALL
            .iter()
            .filter(|c| c.applies.contains(&Level::X5n) && c.status == Status::Todo)
            .count();
        assert_eq!(
            x5n.checks.len(),
            RUNS.iter().filter(|(c, _)| c.applies.contains(&Level::X5n)).count()
                + todo
                + JUDGES.iter().filter(|(c, _)| c.applies.contains(&Level::X5n)).count()
                + SET_JUDGES
                    .iter()
                    .filter(|(c, _)| c.applies.contains(&Level::X5n))
                    .count()
                // x4.encrypt.absent and x4.pdf-reference.conforms, which X-5n inherits.
                + 2
        );
        // Its checks still to be written are not evaluated, so the file cannot pass yet.
        let unwritten = x5n
            .checks
            .iter()
            .filter(|c| c.outcome == Outcome::NotEvaluated("not written yet".into()));
        assert_eq!((unwritten.count(), x5n.verdict), (todo, Verdict::Fail));
        let x4 = RUNS.iter().filter(|(c, _)| c.applies.contains(&Level::X4)).count()
            + JUDGES.iter().filter(|(c, _)| c.applies.contains(&Level::X4)).count();
        // And the two judged from tiers 1 to 5's findings: x4.encrypt.absent and x4.pdf-reference.conforms.
        assert_eq!(verify("x4-good.pdf", &bytes, Level::X4).checks.len(), x4 + 2);
    }

    #[test]
    fn a_check_that_returns_is_left_alone() {
        let found = guarded(X4_TRAPPED_PRESENT, &|| {
            vec![Finding::new(X4_TRAPPED_PRESENT, "missing")]
        });
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].check.id, "x4.trapped.present");
    }

    fn boom(payload: impl std::any::Any + Send) -> Vec<Finding> {
        std::panic::panic_any(payload)
    }

    #[test]
    fn a_panic_becomes_an_internal_finding() {
        let said = |found: Vec<Finding>| {
            assert_eq!((found.len(), found[0].check.id), (1, "internal"));
            found[0].message.clone()
        };
        let prefix = "check x4.trapped.present failed inside proof-pdf: ";
        assert_eq!(
            said(guarded(X4_TRAPPED_PRESENT, &|| boom("static"))),
            format!("{prefix}static")
        );
        assert_eq!(
            said(guarded(X4_TRAPPED_PRESENT, &|| boom(String::from("owned")))),
            format!("{prefix}owned")
        );
        assert_eq!(said(guarded(X4_TRAPPED_PRESENT, &|| boom(7_u8))), prefix);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_refused() {
        let report = verify("x.pdf", b"%PDF-1.7", Level::X4);
        assert_eq!(
            (report.verdict, report.structure),
            (Verdict::Refused, Structure::Refused)
        );
        assert_eq!(report.findings[0].check.id, "file.startxref");
    }

    #[test]
    fn a_damaged_object_makes_the_file_damaged() {
        let body = b"%PDF-1.7\n1 0 obj\n<< /A 2 0 R >>\nendobj\n2 0 obj\n[1\nendobj\n";
        let mut file = body.to_vec();
        file.extend(format!("xref\n0 3\n0000000000 65535 f\r\n0000000009 00000 n\r\n0000000039 00000 n\r\ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", body.len()).as_bytes());
        let report = verify("x.pdf", &file, Level::X4);
        assert_eq!((report.verdict, report.structure), (Verdict::Fail, Structure::Damaged));
        assert_eq!(report.findings[0].check.id, "object.damaged");
    }

    #[test]
    fn the_genuine_fixture_is_sound() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/pdfs/x4-good.pdf"
        ))
        .unwrap();
        let report = verify("x4-good.pdf", &bytes, Level::X4);
        assert_eq!(
            (report.verdict, report.structure),
            (Verdict::Pass, Structure::Sound),
            "{:#?}",
            report.findings
        );
    }
}
