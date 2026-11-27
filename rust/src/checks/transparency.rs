//! Transparency at PDF/X-1a and PDF/X-3: banned outright by 15930-4 and -6 6.16, and by the owner's decision (L-Q3)
//! at X-1a:2001 and X-3:2002 too, which a PDF 1.3 reader would print opaque (15930-3 Annex D). What a page uses is
//! judged, as tier 7 reports it: soft masks, constant opacity and blend modes from the `ExtGState`s it sets, soft-masked
//! images it draws, and transparency groups on the forms it draws (a page's own group is allowed, Q19).

use super::Settings;
use super::info::per_level;
use crate::document::Document;
use crate::findings::{
    Finding, Location, X13_TRANSPARENCY_EXTGSTATE_VALUES, X13_TRANSPARENCY_NO_GROUP, X13_TRANSPARENCY_SMASK,
};
use crate::graphics::{self, Event};
use std::collections::BTreeSet;

/// The clause each level's ban rests on: the data-structure clauses at the 2001 and 2002 levels (L-Q3), 6.16 after.
const BAN: [&str; 4] = ["6.2", "6.1", "6.16", "6.16"];

/// One finding of `check` per reason and place tier 7 reported transparency for, of the reasons `wanted`.
fn reported(
    doc: &Document,
    settings: &Settings,
    wanted: &[&str],
    check: &'static crate::findings::Check,
) -> Vec<Finding> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for event in graphics::walk(doc) {
        if let Event::Transparency { reason, frames } = event
            && wanted.contains(&reason)
            && seen.insert((reason, format!("{frames:?}")))
        {
            let finding = Finding::new(check, format!("a print element uses transparency: {reason}")).at(frames);
            out.push(per_level(finding, settings, BAN));
        }
    }
    out
}

/// No soft mask: an `ExtGState` `/SMask` other than `/None`, or a soft-masked image.
pub(super) fn smask(doc: &Document, settings: &Settings) -> Vec<Finding> {
    reported(doc, settings, &["soft mask"], X13_TRANSPARENCY_SMASK)
}

/// `ExtGState` `/BM` is `Normal` or `Compatible`, and `/CA` and `/ca` are 1.
pub(super) fn extgstate_values(doc: &Document, settings: &Settings) -> Vec<Finding> {
    reported(
        doc,
        settings,
        &["opacity", "blend mode"],
        X13_TRANSPARENCY_EXTGSTATE_VALUES,
    )
}

/// 15930-4 and -6 6.16: no form `XObject` a page draws is a transparency group.
pub(super) fn no_group(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for event in graphics::walk(doc) {
        if let Event::Group { frames, .. } = event
            && matches!(frames.last(), Some(Location::Object(..)))
        {
            let finding = Finding::new(X13_TRANSPARENCY_NO_GROUP, "a form XObject is a transparency group").at(frames);
            out.push(per_level(finding, settings, BAN));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::test_doc::paged;
    use crate::findings::Level;

    fn cited(findings: Vec<Finding>) -> Vec<String> {
        findings
            .into_iter()
            .map(|f| f.clause.map_or_default(|c| format!("{}:{}", c.standard, c.number)))
            .collect()
    }

    #[test]
    fn what_a_page_uses() {
        let page = "/Resources << /ExtGState << /A << /ca 0.5 /SMask << /S /Luminosity >> >> >> >>";
        // Set twice in one place: one finding per reason and place.
        let doc = paged(&[(page, "/A gs /A gs 0 0 1 1 re f")], "", &[]);
        assert_eq!(cited(smask(&doc, &Level::X1a2001.into())), ["iso-15930-1:6.2"]);
        assert_eq!(
            cited(extgstate_values(&doc, &Level::X32002.into())),
            ["iso-15930-3:6.1"]
        );
        assert_eq!(cited(no_group(&doc, &Level::X32003.into())), [""; 0]);
        let form = b"<< /Type /XObject /Subtype /Form /BBox [0 0 1 1] /Group << /S /Transparency >> /Length 0 >>\nstream\n\nendstream";
        let grouped = paged(
            &[(
                "/Group << /S /Transparency >> /Resources << /XObject << /F 9 0 R >> >>",
                "/F Do",
            )],
            "",
            &[(9, form.to_vec())],
        );
        assert_eq!(cited(no_group(&grouped, &Level::X1a2003.into())), ["iso-15930-4:6.16"]);
    }
}
