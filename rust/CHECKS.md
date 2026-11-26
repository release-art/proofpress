# Checks

Generated from `rust/checks.toml` by `rust/build.rs`; `cargo test` fails while this copy is stale (refresh it with `bin/rust/check.sh --fix`).

| Id | Title | Clauses | Severity | Applies | Status |
|---|---|---|---|---|---|
| `x4.pdf-reference.conforms` | Valid PDF as the PDF 1.6 Reference defines it | iso-15930-7:6.1.1, iso-15930-7:B | error | x4, x4p | todo |
| `x4.pre-separated.absent` | Pages are not pre-separated plates | iso-15930-7:6.1.2 | error | x4, x4p | implemented |
| `x4.complete.resources-inside` | Every resource a print element needs is in the file | iso-15930-7:6.3 | error | x4, x4p | todo |
| `x4.complete.single-condition` | All print data is prepared for one printing condition | iso-15930-7:6.3, iso-15930-7:6.4.1 | error | x4, x4p | skip: Intent, not a property of the file: the colour rows are its checkable part |
| `x4.output-intent.channels` | The printing condition has 1, 3 or 4 channels | iso-15930-7:6.4.1 | error | x4, x4p | implemented |
| `x4.output-intent.present` | A GTS_PDFX output intent exists | iso-15930-7:6.4.2.1 | error | x4, x4p | implemented |
| `x4.output-intent.unique` | Only one output intent is GTS_PDFX | iso-15930-7:6.4.2.1 | error | x4, x4p | implemented |
| `x4.output-intent.identifier-present` | The PDF/X intent names its condition identifier | iso-15930-7:6.4.2.1 | error | x4, x4p | implemented |
| `x4.output-intent.identifier-text-string` | The condition identifier is a text string | iso-15930-7:6.4.2.1 | error | x4, x4p | implemented |
| `x4.output-intent.identifier-registered` | The condition identifier names a registered condition | iso-15930-7:6.4.2.1 | warning | x4, x4p | implemented |
| `x4.output-intent.registry-name-color-org` | An ICC-registered condition gives the ICC registry | iso-15930-7:6.4.2.1 | error | x4, x4p | implemented |
| `x4.output-intent.identifier-in-registry` | The identifier exists in the named registry | iso-15930-7:6.4.2.1 | warning | x4, x4p | implemented |
| `x4.output-intent.registry-name-url` | A non-ICC registry name is a URL | iso-15930-7:6.4.2.1 | warning | x4, x4p | implemented |
| `x4.output-intent.dest-profile-present` | The PDF/X intent embeds its profile | iso-15930-7:6.4.2.1 | error | x4 | implemented |
| `x4.output-intent.no-dest-profile-ref` | The PDF/X intent has no external profile reference | iso-15930-7:6.4.2.1 | error | x4 | implemented |
| `x4.output-intent.profile-class-prtr` | The intent profile is an output-device profile | iso-15930-7:6.4.2.1 | error | x4, x4p | implemented |
| `x4.output-intent.profile-version` | The intent profile is ICC version 2 or 4 | iso-15930-7:6.4.2.1 | error | x4, x4p | implemented |
| `x4.output-intent.output-condition` | The PDF/X intent describes its condition for people | iso-15930-7:6.4.2.1 | warning | x4, x4p | implemented |
| `x4.colour.device-space-matches-intent` | Device colour matches the output intent's family | iso-15930-7:6.4.3.2, iso-15930-7:6.4.2.3 | error | x4, x4p | implemented |
| `x4.icc.profile-version` | ICCBased profiles are ICC version 2 or 4 | iso-15930-7:6.4.3.3 | error | x4, x4p | implemented |
| `x4.icc.overprint-mode-zero` | Overprinted four-channel ICC colour uses overprint mode 0 | iso-15930-7:6.4.3.3 | error | x4, x4p | implemented |
| `x4.icc.cmyk-not-intent-profile` | Four-channel ICC colour does not reuse the intent profile | iso-15930-7:6.4.3.3 | error | x4, x4p | implemented |
| `x4.spot.colorants-entry` | Every DeviceN spot has a Colorants entry | iso-15930-7:6.4.3.4 | error | x4, x4p | implemented |
| `x4.spot.consistent-definition` | A spot name has one definition throughout | iso-15930-7:6.4.3.4 | error | x4, x4p | implemented |
| `x4.spot.colorants-match-devicen` | Colorants entries agree with their DeviceN space | iso-15930-7:6.4.3.4 | warning | x4, x4p | implemented |
| `x4.spot.confusing-names` | Spot names do not imitate process or RGB inks | iso-15930-7:6.4.3.4 | warning | x4, x4p | implemented |
| `x4.spot.process-colorspace` | A DeviceN process space matches the output intent | iso-15930-7:6.4.3.4 | error | x4, x4p | implemented |
| `x4.spot.no-dotgain` | NChannel mixing hints carry no dot gain | iso-15930-7:6.4.3.4 | error | x4, x4p | implemented |
| `x4.spot.printing-order-consistent` | Printing orders agree across the file | iso-15930-7:6.4.3.4 | error | x4, x4p | implemented |
| `x4.spot.solidities-consistent` | Each ink has one solidity across the file | iso-15930-7:6.4.3.4 | error | x4, x4p | implemented |
| `x4.spot.mixing-hints-informed` | Mixing hints omit default values | iso-15930-7:6.4.3.4 | warning | x4, x4p | skip: Whether a value was chosen with care cannot be read from the file |
| `x4.spot.alternate-space-restricted` | Spot alternate spaces follow the colour rules | iso-15930-7:6.4.3.4 | error | x4, x4p | implemented |
| `x4.font.embedded` | Fonts used for text are embedded | iso-15930-7:6.5.1, iso-15930-7:6.24 | error | x4, x4p | implemented |
| `x4.font.glyphs-present` | Embedded fonts have every glyph they show | iso-15930-7:6.5.1 | error | x4, x4p | implemented |
| `x4.font.no-notdef` | Text never shows the .notdef glyph | iso-15930-7:6.5.2 | error | x4, x4p | implemented |
| `x4.font.widths-consistent` | Declared widths match the font program | iso-15930-7:6.5.3 | error | x4, x4p | implemented |
| `x4.font.truetype-nonsymbolic-encoding` | Non-symbolic TrueType uses MacRoman or WinAnsi | iso-15930-7:6.5.4 | error | x4, x4p | implemented |
| `x4.font.truetype-nonsymbolic-no-differences` | Non-symbolic TrueType has no Differences | iso-15930-7:6.5.4 | error | x4, x4p | implemented |
| `x4.font.truetype-symbolic-no-encoding` | Symbolic TrueType has no Encoding | iso-15930-7:6.5.4 | error | x4, x4p | implemented |
| `x4.font.truetype-symbolic-one-cmap` | Symbolic TrueType has exactly one cmap subtable | iso-15930-7:6.5.4 | error | x4, x4p | implemented |
| `x4.names.utf8` | Font and separation names are UTF-8 | iso-15930-7:6.6 | error | x4, x4p | implemented |
| `x4.names.hex-escaped` | Bytes outside printable ASCII in those names are escaped | iso-15930-7:6.6 | error | x4, x4p | implemented |
| `x4.names.other-recommended` | Other names are UTF-8 and escaped too | iso-15930-7:6.6 | warning | x4, x4p | todo |
| `x4.xobject.no-opi` | XObjects carry no OPI | iso-15930-7:6.7 | error | x4, x4p | implemented |
| `x4.xobject.no-ref` | Form XObjects are not reference XObjects | iso-15930-7:6.7 | error | x4, x4p | implemented |
| `x4.filespec.embedded` | File specifications embed their file | iso-15930-7:6.7 | error | x4 | implemented |
| `x4.stream.no-external-f` | No stream points at external data | iso-15930-7:6.7 | error | x4, x4p | implemented |
| `x4.filter.no-lzw` | LZW compression is not used | iso-15930-7:6.8 | error | x4, x4p | implemented |
| `x4.filter.no-crypt` | The Crypt filter is not used | iso-15930-7:6.8 | error | x4, x4p | implemented |
| `x4.filter.standard-only` | Only standard filters are used | iso-15930-7:6.8 | error | x4, x4p | implemented |
| `x4.image.dct-valid` | JPEG data is valid | iso-15930-7:6.8 | error | x4, x4p | implemented |
| `x4.image.jpx-valid` | JPEG 2000 data is valid | iso-15930-7:6.8, iso-15930-7:6.27 | error | x4, x4p | implemented |
| `x4.image.jbig2-valid` | JBIG2 data is valid | iso-15930-7:6.8 | error | x4, x4p | implemented |
| `x4.trapped.present` | XMP states whether the file is trapped | iso-15930-7:6.9 | error | x4, x4p | implemented |
| `x4.trapped.true-or-false` | The trapped state is True or False | iso-15930-7:6.9 | error | x4, x4p | implemented |
| `x4.trapped.trapnet-implies-true` | TrapNet annotations need the trapped state True | iso-15930-7:6.9 | error | x4, x4p | implemented |
| `x4.trapped.truthful` | The trapped state is true of the whole file | iso-15930-7:6.9 | error | x4, x4p | skip: Whether a file is really trapped cannot be read from it |
| `x4.trapnet.font-fauxing-empty` | TrapNet font fauxing is absent or empty | iso-15930-7:6.9 | error | x4, x4p | implemented |
| `x4.trapnet.pcm-matches-intent` | TrapNet process colour model matches the intent | iso-15930-7:6.9 | error | x4, x4p | implemented |
| `x4.xmp.present` | The catalog has document metadata | iso-15930-7:6.10.1 | error | x4, x4p | implemented |
| `x4.xmp.conforms` | Document metadata is valid XMP | iso-15930-7:6.10.1 | error | x4, x4p | implemented |
| `x4.xmp.namespace-prefixes` | XMP uses the required prefixes for its namespaces | iso-15930-7:6.10.1, iso-15930-7:6.11 | error | x4, x4p | implemented |
| `x4.xmp.unfiltered` | Metadata streams are not filtered | iso-15930-7:6.10.1 | error | x4, x4p | implemented |
| `x4.info.xmp-analogue-present` | Info entries with an XMP equivalent are in XMP | iso-15930-7:6.10.2 | error | x4, x4p | implemented |
| `x4.info.xmp-analogue-equal` | Info entries equal their XMP equivalents | iso-15930-7:6.10.2 | error | x4, x4p | implemented |
| `x4.info.unlisted-not-in-predefined-schema` | Other Info entries stay out of predefined schemas | iso-15930-7:6.10.2 | error | x4, x4p | implemented |
| `x4.info.pdfx-version-pdfdoc` | Info GTS_PDFXVersion is PDFDocEncoded | iso-15930-7:6.10.2 | error | x4, x4p | implemented |
| `x4.info.author-single-creator` | With Info Author, dc:creator has one item | iso-15930-7:6.10.2 | error | x4, x4p | implemented |
| `x4.xmp.no-bytes-encoding-attrs` | XMP packet headers have no bytes or encoding attribute | iso-15930-7:6.10.3 | error | x4, x4p | implemented |
| `x4.xmp.document-id` | XMP has a document ID | iso-15930-7:6.10.4 | error | x4, x4p | implemented |
| `x4.xmp.version-id` | XMP has a version ID | iso-15930-7:6.10.4 | error | x4, x4p | implemented |
| `x4.xmp.rendition-class` | XMP has a rendition class | iso-15930-7:6.10.4 | error | x4, x4p | implemented |
| `x4.xmp.document-id-uuid` | The document ID is a UUID URI | iso-15930-7:6.10.4 | warning | x4, x4p | implemented |
| `x4.xmp.create-date` | XMP has a creation date | iso-15930-7:6.10.4 | error | x4, x4p | implemented |
| `x4.xmp.modify-date` | XMP has a modification date | iso-15930-7:6.10.4 | error | x4, x4p | implemented |
| `x4.xmp.metadata-date` | XMP has a metadata date | iso-15930-7:6.10.4 | error | x4, x4p | implemented |
| `x4.xmp.title` | XMP has a title | iso-15930-7:6.10.4 | error | x4, x4p | implemented |
| `x4.xmp.agent-names` | XMP names the creator tool and producer | iso-15930-7:6.10.4 | warning | x4, x4p | implemented |
| `x4.trailer.id` | The trailer has a file ID | iso-15930-7:6.10.4 | error | x4, x4p | implemented |
| `x4.xmp.well-formed` | Every XMP packet is well-formed RDF/XML | iso-15930-7:6.10.6 | error | x4, x4p | implemented |
| `x4.identity.version-present` | XMP declares the PDF/X version | iso-15930-7:6.11, iso-15930-7:A.3 | error | x4, x4p | implemented |
| `x4.identity.version-value` | The PDF/X version value is exact | iso-15930-7:6.11, iso-15930-7:A.3 | error | x4, x4p | implemented |
| `x4.identity.pdfxid-only-version` | The pdfxid namespace holds only the version | iso-15930-7:6.11 | error | x4, x4p | implemented |
| `x4.identity.no-info-conformance` | Info has no GTS_PDFXConformance | iso-15930-7:6.11 | error | x4, x4p | implemented |
| `x4.boxes.trim-or-art` | Each page has a TrimBox or an ArtBox | iso-15930-7:6.12 | error | x4, x4p | implemented |
| `x4.boxes.not-both` | No page has both TrimBox and ArtBox | iso-15930-7:6.12 | error | x4, x4p | implemented |
| `x4.boxes.within-bleed` | The trim or art box lies within the BleedBox | iso-15930-7:6.12 | error | x4, x4p | implemented |
| `x4.boxes.within-crop` | Page boxes lie within the CropBox | iso-15930-7:6.12 | error | x4, x4p | implemented |
| `x4.boxes.within-media` | Page boxes lie within the MediaBox | iso-15930-7:6.12 | error | x4, x4p | implemented |
| `x4.boxes.prefer-trim` | TrimBox is used rather than ArtBox | iso-15930-7:6.12 | warning | x4, x4p | implemented |
| `x4.extgstate.no-tr` | Graphics states have no transfer function | iso-15930-7:6.13 | error | x4, x4p | implemented |
| `x4.extgstate.no-htp` | Graphics states have no halftone phase | iso-15930-7:6.13 | error | x4, x4p | implemented |
| `x4.extgstate.tr2-default` | TR2, if present, is Default | iso-15930-7:6.13 | error | x4, x4p | implemented |
| `x4.halftone.transfer-function` | Halftones carry transfer functions only where required | iso-15930-7:6.13 | error | x4, x4p | implemented |
| `x4.halftone.type-1-or-5` | Halftones are type 1 or 5 | iso-15930-7:6.13 | error | x4, x4p | implemented |
| `x4.halftone.no-name` | Halftones have no name | iso-15930-7:6.13 | error | x4, x4p | implemented |
| `x4.xobject.no-postscript` | No PostScript XObjects | iso-15930-7:6.14 | error | x4, x4p | implemented |
| `x4.xobject.no-subtype2-ps` | No form XObject is PostScript underneath | iso-15930-7:6.14 | error | x4, x4p | implemented |
| `x4.encrypt.absent` | The file is not encrypted | iso-15930-7:6.15 | error | x4, x4p | implemented |
| `x4.perms.ur-only` | Permissions hold only usage rights | iso-15930-7:6.15 | error | x4, x4p | implemented |
| `x4.image.no-default-for-printing` | No image alternate is the default for printing | iso-15930-7:6.16 | error | x4, x4p | implemented |
| `x4.image.alternates-same-content` | Image alternates show the same picture | iso-15930-7:6.16 | error | x4, x4p | skip: Whether two images show the same area of one master image, at another resolution or in another colour space, needs a similarity judgement, not a reading of the file |
| `x4.annot.outside-bleed` | Annotations lie outside the bleed | iso-15930-7:6.17 | error | x4, x4p | implemented |
| `x4.annot.printermark-outside-trim` | Printer's marks lie outside the trim | iso-15930-7:6.17 | error | x4, x4p | implemented |
| `x4.actions.absent` | The file has no actions | iso-15930-7:6.18, iso-15930-7:6.2 | error | x4, x4p | implemented |
| `x4.javascript.absent` | The file has no JavaScript | iso-15930-7:6.18, iso-15930-7:6.2 | error | x4, x4p | implemented |
| `x4.content.known-operators` | Content uses only PDF 1.6 operators | iso-15930-7:6.19, iso-15930-7:B | error | x4, x4p | implemented |
| `x4.content.no-bx-ex` | Content avoids BX and EX | iso-15930-7:6.19 | warning | x4, x4p | implemented |
| `x4.transparency.group-cs-restricted` | Transparency group colour spaces follow the colour rules | iso-15930-7:6.20 | error | x4, x4p | implemented |
| `x4.transparency.blend-modes` | Blend modes are standard | iso-15930-7:6.20 | error | x4, x4p | implemented |
| `x4.viewer-prefs.areas` | Viewer areas and clips are MediaBox or BleedBox | iso-15930-7:6.21 | error | x4, x4p | implemented |
| `x4.names.no-alternate-presentations` | No alternate presentations | iso-15930-7:6.22 | error | x4, x4p | implemented |
| `x4.page.no-pres-steps` | No page has presentation steps | iso-15930-7:6.22 | error | x4, x4p | implemented |
| `x4.intent.known-values` | Rendering intents are standard | iso-15930-7:6.23, iso-15930-7:6.13 | error | x4, x4p | implemented |
| `x4.oc.config-name-present` | Optional-content configurations are named | iso-15930-7:6.24 | error | x4, x4p | implemented |
| `x4.oc.config-name-unique` | Optional-content configuration names are unique | iso-15930-7:6.24 | error | x4, x4p | implemented |
| `x4.oc.order-complete` | An Order lists every optional-content group, or none | iso-15930-7:6.24 | error | x4, x4p | implemented |
| `x4.oc.no-as` | Optional-content configurations have no AS | iso-15930-7:6.24 | error | x4, x4p | implemented |
| `x4.limits.lexical` | Numbers, strings and names stay within PDF 1.6 limits | iso-15930-7:6.25 | error | x4, x4p | todo |
| `x4.limits.objects` | The indirect-object count stays within PDF 1.6 limits | iso-15930-7:6.25 | error | x4, x4p | implemented |
| `x4.limits.graphics` | Graphics-state nesting, DeviceN and CIDs stay within limits | iso-15930-7:6.25 | error | x4, x4p | todo |
| `x4.acroform.no-xfa` | Forms have no XFA | iso-15930-7:6.26 | error | x4, x4p | implemented |
| `x4.jpx.baseline` | JPEG 2000 uses the JPX format, baseline features only | iso-15930-7:6.27 | error | x4, x4p | implemented |
| `x4.jpx.channels` | JPEG 2000 has 1, 3 or 4 channels | iso-15930-7:6.27 | error | x4, x4p | implemented |
| `x4.jpx.one-colour-space` | JPEG 2000 declares at most one colour space | iso-15930-7:6.27 | error | x4, x4p | implemented |
| `x4.jpx.no-ciejab` | JPEG 2000 does not use CIEJab | iso-15930-7:6.27 | error | x4, x4p | implemented |
| `x4.jpx.cmyk-as-device` | Profile-less CMYK JPEG 2000 is judged as DeviceCMYK | iso-15930-7:6.27 | error | x4, x4p | implemented |
| `x4.jpx.bit-depth` | JPEG 2000 bit depth is 1, 8 or 16 | iso-15930-7:6.27 | error | x4, x4p | implemented |
| `x4.jpx.uniform-depth` | JPEG 2000 channels share one bit depth | iso-15930-7:6.27 | error | x4, x4p | implemented |
| `x4p.output-intent.dest-profile-ref` | The PDF/X intent references its external profile | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.output-intent.no-dest-profile` | The PDF/X intent embeds no profile | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.ref.profile-name` | The profile reference names the profile | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.ref.urls` | The profile reference lists URL file specifications | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.ref.urls-no-ef-platform` | Those URLs embed nothing and name no platform paths | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.ref.checksum` | The profile reference has a 16-byte checksum | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.ref.profile-cs` | The profile reference states GRAY, RGB or CMYK | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.ref.icc-version` | The profile reference states the ICC version | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.ref.no-colorant-table` | The profile reference has no colorant table | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.fileset.checksum-matches` | The external profile matches the checksum | iso-15930-7:A.1, iso-15930-7:A.2 | error | x4p | todo |
| `x4p.fileset.fields-match` | The external profile matches the reference's fields | iso-15930-7:A.2 | error | x4p | todo |
| `x4p.fileset.profile-valid` | The external profile meets the output-intent rules | iso-15930-7:A.1 | error | x4p | todo |
| `x5.identification-value` | XMP declares an X-5 level | iso-15930-8:6.2 | error | x5g, x5n, x5pg | todo |
| `x5.self-contained-is-x4` | A self-contained file claims X-4, not X-5 | iso-15930-8:5 | error | x5g, x5pg | todo |
| `x5.g-ref-present` | At least one reference XObject exists | iso-15930-8:8.2 | error | x5g, x5pg | todo |
| `x5.g-proxy-is-form` | External content is placed through a form proxy | iso-15930-8:8.2 | error | x5g, x5pg | todo |
| `x5.g-ref-id-required` | Every reference has a file ID | iso-15930-8:8.2 | error | x5g, x5pg | todo |
| `x5.g-ref-filespec-external` | A reference's file may be outside the file | iso-15930-8:8.2, iso-15930-7:6.7 | error | x5g, x5pg | todo |
| `x5.g-proxy-metadata` | A proxy form has metadata | iso-15930-8:8.3 | error | x5g, x5pg | todo |
| `x5.g-proxy-rendition-of` | A proxy's XMP says which document it renders | iso-15930-8:8.3 | error | x5g, x5pg | todo |
| `x5.g-target-conformance-level` | Targets are at an allowed PDF/X level | iso-15930-8:8.2 | error | x5g, x5pg | todo |
| `x5.g-target-conforms` | Targets conform to the level they claim | iso-15930-8:8.2 | error | x5g, x5pg | todo |
| `x5.g-no-circular-refs` | References never lead back to a visited file | iso-15930-8:8.2 | error | x5g, x5pg | todo |
| `x5.g-target-xmp-ids` | Targets carry XMP document, version and rendition IDs | iso-15930-8:8.3 | error | x5g, x5pg | todo |
| `x5.g-document-id-uuid` | A target's document ID is a UUID URI | iso-15930-8:8.3 | warning | x5g, x5pg | todo |
| `x5.g-target-identity` | Reference IDs match their target | iso-15930-8:8.4 | error | x5g, x5pg | todo |
| `x5.g-target-version-policy` | A target of another version is accepted only by choice | iso-15930-8:8.4 | error | x5g, x5pg | todo |
| `x5.g-target-page-exists` | A reference's page exists in its target | iso-15930-8:8.2, iso-32000-1:8.10.4 | error | x5g, x5pg | todo |
| `x5.g-same-printing-condition` | Container and targets share a printing condition | iso-15930-8:8.5 | error | x5g, x5pg | todo |
| `x5.g-target-no-ocg` | Targets have no optional content | iso-15930-8:8.7 | error | x5g, x5pg | todo |
| `x5.g-proxy-bbox-target-page` | A proxy's BBox is in the target page's space | iso-15930-8:8.5 | error | x5g, x5pg | todo |
| `x5.n-profile-iso15076` | The n-colorant profile is ICC version 4 | iso-15930-8:7.1 | error | x5n | todo |
| `x5.n-trapnet-pcm` | TrapNet process colour model is DeviceN | iso-15930-8:7.1 | error | x5n | todo |
| `x5.n-trapnet-separations` | TrapNet lists every output-intent colorant | iso-15930-8:7.1 | error | x5n | todo |
| `x5.n-urls-ef-allowed` | A profile URL may embed the profile | iso-15930-8:7.2 | error | x5n | todo |
| `x5.n-profilecs-xclr` | The profile colour space is an n-colour signature | iso-15930-8:7.2 | error | x5n | todo |
| `x5.n-3clr-4clr-not-rgb-cmyk` | Three or four colorants do not stand for RGB or CMYK | iso-15930-8:7.2 | error | x5n | todo |
| `x5.n-subtractive` | The printing condition is subtractive | iso-15930-8:7.2 | error | x5n | todo |
| `x5.n-colorant-table-present` | The profile reference has a colorant table | iso-15930-8:7.2 | error | x5n | todo |
| `x5.n-colorant-table-count` | The colorant table matches the channel count | iso-15930-8:7.2 | error | x5n | todo |
| `x5.n-colorant-table-matches-profile` | The colorant table matches the profile | iso-15930-8:7.2 | error | x5n | todo |
| `x5.n-devicegray-needs-black` | DeviceGray needs a Black colorant | iso-15930-8:7.3 | error | x5n | todo |
| `x5.n-devicecmyk-needs-cmyk` | DeviceCMYK needs all four process colorants | iso-15930-8:7.3 | error | x5n | todo |
| `x5.n-no-devicergb` | DeviceRGB is not used | iso-15930-8:7.3 | error | x5n | todo |
| `x5.n-devicen-subtype` | DeviceN attributes are not NChannel | iso-15930-8:7.3 | error | x5n | todo |
| `x5.n-devicen-no-process` | DeviceN attributes have no Process | iso-15930-8:7.3 | error | x5n | todo |
| `x5.n-transparency-group-cs` | Pages with transparency declare a group colour space | iso-15930-8:7.3 | error | x5n | todo |
| `x5.reader.writer-level` | A writer produces at least one X-5 level | iso-15930-8:5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.reader-levels` | A reader handles its levels plus X-4 and X-4p | iso-15930-8:5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.documentation` | Products name the levels they support | iso-15930-8:5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.parse-all` | Readers parse all PDF and may ignore print flags | iso-15930-8:5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.file-set-rendering` | File sets render as the PDF Reference says | iso-15930-8:5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.old-features` | Features from before PDF 1.6 may be ignored | iso-15930-8:5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.cmm-capabilities` | Colour management needed for n-colour conditions | iso-15930-8:7.2 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.colorant-names` | Spot names matching the colorant table are those colorants | iso-15930-8:7.3 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.device-mapping` | Device gray and CMYK map onto the named colorants | iso-15930-8:7.3 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.cycle-detection` | Readers detect circular references | iso-15930-8:8.2 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.finding-targets` | How a reader finds targets is open | iso-15930-8:8.4 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.matching-workflow` | Readers treat an ID match as exact or another version | iso-15930-8:8.4 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.use-target-data` | Rendering uses all target data | iso-15930-8:8.5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.target-fonts` | Each file's text uses that file's fonts | iso-15930-8:8.5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.cross-file-blending` | Overprint and transparency across files look like one file | iso-15930-8:8.5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.target-placement` | Target pages are placed by their rotated MediaBox | iso-15930-8:8.5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.profile-conversion` | Assembly converts between differing profiles | iso-15930-8:8.5 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `x5.reader.trapped-scope` | pdf:Trapped says nothing about targets | iso-15930-8:8.6 | error | x5g, x5pg, x5n | skip: A rule on readers, writers or workflows, not a property of a file (ISO 15930-8, register §4) |
| `tac` | Peak total ink stays under the limit |  | error | print | todo |
| `overprint` | Overprint is used on purpose |  | warning | print | implemented |
| `white-overprint` | White is never set to overprint |  | error | print | implemented |
| `colour-space` | Colour spaces suit the job's colour mode |  | error | print | implemented |
| `rgb-only` | An RGB job paints only RGB |  | error | print | implemented |
| `gray-only` | A gray job paints only gray |  | error | print | implemented |
| `k-only-text` | Text is 100% black only |  | error | print | implemented |
| `rich-black` | Small text is not in rich black |  | warning | print | implemented |
| `spot-ink` | Spot inks are on the allowed list |  | error | print | implemented |
| `spot-definition` | A spot ink has one definition |  | error | print | implemented |
| `icc-profile` | Embedded ICC profiles are sound and suit the job |  | error | print | implemented |
| `blend-space` | Transparency declares its blend space |  | error | print | implemented |
| `image-dpi` | Images meet the resolution floor |  | error | print | implemented |
| `image-dpi-max` | Images do not exceed the resolution ceiling |  | warning | print | implemented |
| `image-stretch` | Images are not stretched unevenly |  | warning | print | implemented |
| `text-size` | Text meets the size floor |  | error | print | implemented |
| `hairline` | Strokes meet the width floor |  | error | print | implemented |
| `contrast` | Text contrasts with its background |  | error | print | implemented |
| `safe-area` | Text stays inside the safe area |  | error | print | implemented |
| `missing-glyph` | Text shows no missing glyphs |  | error | print | implemented |
| `font-embedded` | Fonts are embedded |  | error | print | implemented |
| `font-type3` | Type 3 fonts are reported |  | warning | print | implemented |
| `geometry` | Pages are the expected trim size |  | error | print | implemented |
| `bleed` | Pages carry the expected bleed |  | error | print | implemented |
| `centring` | The trim is centred in the bleed |  | error | print | implemented |
| `artwork-bleed` | Artwork reaches into the bleed |  | error | print | implemented |
| `outside-bleed` | Nothing is painted beyond the bleed |  | error | print | implemented |
| `outside-media` | Nothing is painted wholly off the page |  | warning | print | implemented |
| `xobject-missing` | Painted XObjects exist |  | error | print | implemented |
| `bleedbox` | Pages with a trim have a BleedBox |  | error | print | todo |
| `boxes` | Each page has a MediaBox |  | error | print | todo |
| `page-size` | Pages share one size |  | error | print | implemented |
| `page-count` | The page count fits the rule |  | error | print | implemented |
| `page-rotate` | Pages are not rotated |  | warning | print | implemented |
| `output-condition` | The output intent names the expected condition |  | error | print | implemented |
| `pdfa` | veraPDF passes the PDF/A flavour |  | error | print | todo |
| `structure` | The file can be read |  | error | print | todo |
| `internal` | The engine itself failed on this file |  | error | x4, x4p, x5g, x5pg, x5n, print | skip: Raised when a check panics, which is always a bug in proof-pdf: no file is meant to trigger it, so it has no fixtures; unit tests cover it |
| `file.header` | The file starts with a %PDF- header in its first 1024 bytes | iso-32000-2:7.5.2 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `file.header-offset` | The header is at the very start of the file | iso-32000-2:7.5.2 | warning | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `file.startxref` | The file ends with startxref and the offset of its cross-reference | iso-32000-2:7.5.5 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `file.xref` | Every cross-reference section can be read | iso-32000-2:7.5.4, iso-32000-2:7.5.8 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `file.trailer` | The trailer names the document catalog | iso-32000-2:7.5.5 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `file.encrypted` | The file is not encrypted | iso-32000-2:7.6 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `file.eof` | The file ends with %%EOF and nothing after it | iso-32000-2:7.5.5 | warning | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `file.limit` | The file stays within proof-pdf's safety limits |  | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `syntax.hex-digits` | Hex strings have an even number of digits | iso-32000-2:7.3.4.3 | warning | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `syntax.octal-escape` | Octal escapes in strings stay within one byte | iso-32000-2:7.3.4.2 | warning | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `syntax.name-escape` | Escapes in names are two hex digits, never 00 | iso-32000-2:7.3.5 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `syntax.duplicate-key` | No dictionary gives a key twice | iso-32000-2:7.3.7 | warning | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `stream.eol` | Stream data is followed by an end of line before endstream | iso-32000-2:7.3.8.1 | warning | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `stream.filter` | Every stream names filters ISO 32000 defines | iso-32000-2:7.4.1 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `content.damaged` | Every content stream can be read to its end | iso-32000-2:7.8.2, iso-32000-2:8.9.7 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `content.operator` | Undefined operators appear only inside BX and EX | iso-32000-2:7.8.2 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `content.compatibility` | BX and EX pair up | iso-32000-2:7.8.2 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `object.damaged` | Every object reachable from the trailer can be read | iso-32000-2:7.3, iso-32000-2:7.3.8.1, iso-32000-2:7.3.10, iso-32000-2:7.5.7 | error | x4, x4p, x5g, x5pg, x5n, print | implemented |
| `object.dangling-reference` | References point at objects that exist | iso-32000-2:7.3.10 | warning | x4, x4p, x5g, x5pg, x5n, print | implemented |
