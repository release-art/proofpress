# How the checks read the standards

Where a standard is unclear, or practice differs from its letter, this is the reading the checks implement. Code, `rust/checks.toml`
and `tests/fixtures/pdfx/expected.toml` cite the entries by id (`X4-Q3`). A reading changes only together with its checks and this page.

Clause numbers are those of the part named; "6.x" alone means the part of the section's heading. The standards themselves are
licensed and are not quoted here.

## Reading files

- A strict parser of our own: no repair, no decryption. Only file-level damage refuses a file. A damaged object or stream leaves a
  gap, the file still fails, and reading inside a stream stops at the first damage.
- A duplicate dictionary key (ISO 32000-2 7.3.7) is a warning, `syntax.duplicate-key`: real files that print correctly carry them.
- Verifying never uses the network. Reference data (the ICC characterization registry's reference names) is built in, with its
  source and date. veraPDF (`--pdfa`) and Ghostscript (`--tac`) run only when asked; a missing tool is a finding for its layer.
- Total ink counts each spot colour as one ink at its tint, measured on Ghostscript's `tiffsep` plates.
- Findings cite a clause; each check declares the clauses it may cite, checked against `rust/clauses/`.

## PDF/X-4 and X-4p (ISO 15930-7)

| Id | Topic (clause) | Reading |
|---|---|---|
| X4-Q1 | Where `Trapped` must be (6.9, 6.10.2, Annex B) | XMP `pdf:Trapped` is required; Info `/Trapped` is judged only when present, for equality |
| X4-Q2 | Pre-separated pages (6.1.2) | Fail only when every page paints one ink and the inks differ across pages |
| X4-Q3 | Print versus non-print (6.1.3, 6.2) | Colour and font rows judge print elements only; bans on keys (OPI, PostScript, `TR`, actions) cover the whole file |
| X4-Q4 | Spot alternates (6.4.3.4) | Follow the colour rules |
| X4-Q5 | `/Process` colour space (6.4.3.4) | A device space of the right family, or an `ICCBased` space of that family |
| X4-Q6 | `dc:creator` (6.10.2) | Exactly one item. No corrigendum to 15930-7:2010 exists |
| X4-Q7 | "Appropriate" dates and title (6.10.4) | Non-empty and syntactically valid; a placeholder title is not a finding |
| X4-Q8 | Unfiltered metadata (6.10.1) | Every `/Metadata` stream |
| X4-Q9 | Halftone transfer functions (6.13) | Allowed only for non-primary colorants inside a Type 5 halftone |
| X4-Q10 | The version value (6.11) | Exact string; a `DestOutputProfileRef` in a file labelled X-4 is reported as X-4 failures |
| X4-Q11 | Actions (6.18) | Every action fails, `GoTo` and `URI` included; `/Dest` passes |
| X4-Q12 | File specifications under X-4p (6.7, A.2) | Every `/URLs` element is exempt from the `/EF` rule, at every level |
| X4-Q13 | Optional content `/Order` (6.24) | Every OCG reachable from the catalog's `/OCGs` |
| X4-Q14 | Implementation limits (6.25) | The PDF 1.6 Reference's Table C.1 only: numbers, strings in content, names, indirect objects, `q` nesting (28), DeviceN components (32), CIDs. Every saved state counts while a page is drawn, forms and tiling patterns included; every `DeviceN` space in the file counts, used or not; a real nearer to 0 than the smallest reads as 0 |
| X4-Q15 | JPEG 2000 (6.27) | One colour specification box. "Baseline" is read as written (ITU-T T.801 M.9.2): the `jpx ` brand and a Reader Requirements box are needed, so plain JP2 files and bare codestreams fail `x4.jpx.baseline` |
| X4-Q16 | Width consistency (6.5.3) | Glyphs actually shown; the tolerance is the rounding of 1/1000 em units |
| X4-Q17 | `/ProfileCS`, `/ICCVersion` (A.2) | Compared as bytes |
| X4-Q18 | Box comparisons (6.12, 6.17) | Inverted rectangles are normalised; one sub-point tolerance shared with the geometry checks |
| X4-Q19 | Blend-mode arrays (6.20) | Every element must be a standard name |
| X4-Q20 | Which DeviceN names are spots (6.4.3.4) | All except Cyan, Magenta, Yellow, Black and `None`; `All` counts as a spot |
| X4-Q21 | A missing `BleedBox` | Not an X-4 rule; a print check outside the `x4.` ids |

Further readings:

- **ICC versions** (6.4.2.1, 6.4.3.3): major version 2 or 4, any minor version, passes; anything else fails.
- **The ICC registry** (6.4.2.1): an empty `/RegistryName` is absent. An identifier under the ICC URL that is not a reference
  name is a warning (`x4.output-intent.identifier-in-registry`); a misspelt color.org URL with an ICC name fails; another
  registry's URL is not judged; "Custom" or an empty identifier is a warning.
- **`DefaultCMYK`** equal to the output profile (6.4.3.3) fails wherever it remaps a painted DeviceCMYK object.
- **Same-named separations** (6.4.3.4) share tint transform and alternate across the separations print elements use (and the
  `/Colorants` of used DeviceN spaces), process names included. `PrintingOrder` and `Solidities` rows are file-wide.
- **Glyphs** (6.5.1): a code is skipped only when every appearance of it is in render mode 3. Predefined CMaps are Adobe's.
- **Dates** (6.10.2) compare as instants in UTC when both carry a zone, as written when neither does, and never match when only one does.
- **Alternate images** (6.16): "the same master image" is a `skip`; no alternate with `/DefaultForPrinting true` is checked.
- **X-4p file sets** (A.1, A.2): the command line verifies a file with the files it refers to (`--file-alone` to verify it
  alone; the Python `_core.verify` always verifies alone). The external profile is looked for at local `/URLs` (relative
  references and `file:` URLs, against the PDF's folder); other schemes are never fetched. The candidate whose MD5 is the
  `/CheckSum` is the profile. No local match fails `x4p.fileset.checksum-matches`, unless a network URL remains, which leaves it
  not evaluated. A file verified alone cannot pass X-4p. `/ProfileName` matches any record of a multi-localized description.

## PDF/X-5 (ISO 15930-8, with Technical Corrigendum 1)

Inheritance (6.1): X-5g takes every X-4 row and X-5pg and X-5n every X-4p row, except those 15930-8 replaces: the version (6.2),
6.7's `Ref` and `/EF` rules at X-5g and X-5pg (8.2), and at X-5n the colour rows of 7.1-7.3 and A.2's profile rows.

| Id | Topic (clause) | Reading |
|---|---|---|
| X5-Q1, Q2 | Must X-5g hold a proxy (5, Table A.1, 8.2) | Every form with `/Ref` is checked; an X-5g file with none is a warning, `x5.g-ref-present` |
| X5-Q3 | `/EF` on a reference's file specification (8.2) | Not required for the target; an embedded target is accepted |
| X5-Q4 | `3CLR`/`4CLR` misuse (7.2) | Judged by the `ColorantTable` names; a length differing from the channel count is a warning |
| X5-Q5 | A target of another version (8.4) | `--target-versions accept\|reject`, with no default; without it, not evaluated |
| X5-Q6 | Property spelling (8.3, 8.4) | `documentID`, `versionID`, `renditionClass`, as the corrigendum spells them; another case is its own finding |
| X5-Q7 | Same printing condition (8.5) | Equal `OutputConditionIdentifier` and `RegistryName`; differing profiles are a warning |
| X5-Q8 | OCGs in targets (8.7) | Banned in X-4, X-4p, X-5g and X-5pg targets; `OCProperties` or any OCG fails |
| X5-Q9 | Targets at the older levels (8.2) | Verified at the level they claim (L-Q10); a plain PDF/X-1 target fails `x5.g-target-conformance-level` |
| X5-Q10 | Alternates under X-5n (7.3) | The device bans reach the alternate spaces print elements use |
| X5-Q11 | "Identical to the output profile" when it is external | Compared with the external file when the set holds it; otherwise not evaluated |
| X5-Q12 | What "uses transparency" covers (7.3) | Any print element on the page |
| X5-Q13 | Proxies inside targets (8.2) | Followed, down to `Limits::target_depth` (16 files) |

Further readings:

- **File sets** (8.2-8.7): a `/Ref`'s `/F` is a file name relative to the container, a URL file specification or an embedded
  file. Each target is read once. A missing target fails `x5.g-target-identity`; a network-only one is not evaluated. A target
  fails `x5.g-target-conforms` on its errors and unjudged checks, not its warnings. `/Page` labels are read as ISO 32000-2 12.4.2 page labels.
- **`/Ref` on anything but a form** is a warning, `x5.g-proxy-is-form` (ISO 32000-1 8.10.4 defines it for forms only). Such a
  `/Ref` is no reference, so its `/F` owes `/EF` under 15930-7 6.7.
- **X-5n** (clause 7): "ISO 15076-1" is ICC 4.2 or later, and the external profile is an output (`prtr`) profile; without the
  profile, `/ICCVersion` alone can show an earlier version. A URL file specification's `/EF` is a candidate profile. The device
  bans cover print elements, the alternates they reach, transparency group spaces and JPEG 2000 data read as `DeviceCMYK`.
  DeviceN `/Process` fails. `x5.n-subtractive` is a `skip`, as nothing in the file records it.
- `x5.self-contained-is-x4` is a `skip` (the same test as `x5.g-ref-present` at X-5g, never true at X-5pg);
  `x5.g-proxy-bbox-target-page` is a `skip` (8.5 says how a reader places a page, not what a file holds).

## PDF/X-1a and X-3 (ISO 15930-1, -3, -4, -6)

Levels `x1a-2001`, `x3-2002`, `x1a-2003`, `x3-2003`. Without `--pdfx`, a file claiming one of them (XMP `pdfxid:GTS_PDFXVersion`,
else Info `/GTS_PDFXVersion` with `/GTS_PDFXConformance`) is verified at it; any other file at X-4. Check ids: `x1a.*` for
X-1a only, `x3.*` for X-3 only, `x13.*` for rules in more than one of these parts that differ from X-4. An `x4.*` check is reused
only where the property and the test are the same; it keeps citing 15930-7, and `checks.toml`'s `cite` maps the clause.

| Id | Topic (clause) | Reading |
|---|---|---|
| L-Q1 | Plain PDF/X-1 (15930-1, OPI references) | Not evaluated: a `PDF/X-1:2001` claim is verified at X-1a:2001 with its conformance key not evaluated |
| L-Q2 | Cross-reference and object streams (15930-1 6.2, the others 6.1) | An error, `x13.structure.xref-table`; a hybrid's `/XRefStm` alone is not judged |
| L-Q3 | Transparency at X-1a:2001 and X-3:2002, which have no transparency clause | An error by 15930-3 Annex D.2's keys (`BM`, `CA`, `ca`, `SMask`, image `SMask`), citing clause 5 and the data-structure clause; what pages use is judged. Form groups are judged at the 2003 levels only; a page group passes |
| L-Q4 | Features newer than the part's PDF version | No blanket version check. Every filter must be one the part's PDF Reference defines (`JPXDecode`, `Crypt`, and `JBIG2Decode` at X-3:2002 fail); 16-bit samples fail; optional content that can hide print content fails and always-on optional content warns |
| L-Q5 | XMP (Annex B, informative) | Not required; at the 2003 levels an XMP modification date that disagrees with Info `/ModDate` is a warning |
| L-Q6 | Reuse of `x4.*` checks | Only where property and test are the same |
| L-Q7 | Id prefixes | As above |
| L-Q8 | A 2001 file read by a 2003 reader (15930-4 clause 5) | Each file is judged by the part it claims |
| L-Q9 | Carried over from X-4 | X4-Q2, X4-Q3 (with each part's own print elements) and X4-Q11 |
| L-Q10 | X-5 targets at these levels | Verified at the level they claim |
| L-Q11 | ICC editions (clause 2 of each part) | Major version 2 only |
| L-Q12 | `Default*` spaces at X-1a | One that remaps a print element fails, at both X-1a levels; one that remaps nothing warns (page resources only) |
| L-Q13 | X-1a:2001 conformance value (15930-1 6.13) | `PDF/X-1a:2001`; the printed spelling is a misprint and fails |
| L-Q14 | `ri`, `/RI` at X-1a:2001 (Annex A only) | Not restricted |
| L-Q15 | ICC `RegistryName` spelling | `http://www.color.org` exactly; at X-3:2003 with or without the final slash |
| L-Q16 | The X-1a printing condition | CMYK; another output profile space fails `x1a.output-intent.cmyk` |
| L-Q17 | Fonts used only in render mode 3 | Need not be embedded, as at X-4 |
| L-Q18 | Filters at X-1a:2001 (6.8) | ASCII encodings anywhere; Flate and RunLength on any stream; DCT on images only; CCITT on 1-bit images only. A thumbnail and an inline image count as images; inline images are not judged by the filter and depth rows |
| L-Q19 | Page transparency groups (2003 parts, 6.16) | A page `/Group` passes |
| L-Q20 | `PrinterMark` annotations at X-1a:2001 and X-3:2002 | Not exempt: every annotation but `TrapNet` lies outside the bleed |
| L-Q21 | Viewer preferences (2003 parts, 6.17) | Restricted when any page has its own `/BleedBox` |
| L-Q22 | File specifications | A `Filespec` dictionary, a value of `/FS` or `/AF`, or a string under `/FS`. Fails at X-3:2002 and the 2003 levels; an embedded one warns at X-1a:2001 |

Further readings:

- **Identification and Info**: Info `/Trapped` is the name `True` or `False`; a string, `Unknown` or another name fails.
  `/CreationDate` and `/ModDate` parse as PDF dates and `/Title` is not empty (X4-Q7). The version equals the part's value
  exactly. A new check whose clause differs by level cites its level's part.
- **Colour**: without an embedded profile, the condition's colour family is the ICC registry entry its identifier names.
  `x1a.colour.print-spaces` judges print elements and the bases of their `Indexed` and `Pattern` spaces;
  `x1a.spot.alternate-cmyk-or-gray` the alternates of every `Separation` and `DeviceN` they reach.
- **Other rows**: `/TR2` fails whatever its value; at the 2001 and 2002 levels only the trim or art box must lie inside the crop
  box; a TrueType or OpenType program whose `OS/2` `fsType` restricts embedding warns.

## Commands other than `verify`

- **`extract`** writes images as stored: JPEG, JPEG 2000 and CCITT data are copied (`.jpg`, `.jp2`, `.tif`), a soft mask is a
  file of its own (`.mask.png`, ...), and other images are decoded (`/Decode` applied) to PNG (gray, RGB) or Deflate TIFF (CMYK)
  with their ICC profile; `Indexed` images in their base space. Lab, `Separation` and `DeviceN` images are listed as failures.
  Vector artwork is grouped in Python from pieces the core writes; a piece's digest is of its content and the resources it names.
- **`preview`** renders with `hayro`, leniently, from the file as `hayro` reads it: a picture judges nothing, and a damaged file
  still gets one. Guides are drawn without anti-aliasing, so their colours are exact.
- **`crop`** and the writer copy streams as stored and re-serialise values (reals as the shortest text that reads back, never an
  exponent). A copy that needs a damaged object is refused, naming it. An Identity `/Crypt` filter is dropped. A resource is kept
  when its name appears anywhere in the page's content, and all are kept when the content cannot be read through.
- **`inspect`** reports a proof profile LittleCMS cannot read as its `contrast_error`, and a missing MediaBox as none.
