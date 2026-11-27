# Note: a catalogue of automatic PDF fixes

Written 2026-11-26 as input to one design question: should proof-pdf gain a **fix library**, a set of fix rules kept, versioned and
updated apart from the verification pipeline? Today proof-pdf never repairs (see Positioning in [`../README.md`](../README.md)). This
note decides nothing. It lists what other tools fix, how they organise it, and which of our checks a fix would make pass.

## Sources and caveats

Read on 2026-11-26. Every row is in our own words.

- **Kura** (AGPL-3.0, C++), commit `0c6fb56` of 2026-11-14: the issue-code list, the profile fix-up catalogue and the engine's pass
  order. Ideas only, by the owner's decision of 2026-11-07: no code, profile text, thresholds or default values are recorded here.
- **callas pdfToolbox 17.1**: the 119 fixup types used by its shipped profiles, and the 64 built-in fixups its PDF/X conversions
  apply, as the product names them. Parameter names for pdfToolbox are partly inferred, since profiles store them by position.
- **Enfocus PitStop**: the Preflight Report Help of October 2025, where each check names the fix or Action that cures it. The
  per-Action manual was not available as text, so Actions that no check names (text, dielines, barcodes) are missing.
- **Adobe Acrobat**: the help pages refused us. Its rows rest on one secondary summary and are the thinnest here.
- **Ghostscript** 10.08 (pdfwrite), **qpdf** 12.4.2, **mutool** clean, **pdfcpu**, **pikepdf**, the **veraPDF** metadata fixer,
  the **PDF Tools** and **Apryse** SDKs, **BentoPDF** and **Hyper Compress**. Rows marked **(K)** come from memory and were not
  checked this time.

Tool codes in `Offered by`: K Kura, T pdfToolbox, P PitStop, A Acrobat, G Ghostscript, Q qpdf, M mutool, V veraPDF, S commercial
SDKs (PDF Tools, Apryse), B BentoPDF and Hyper Compress, R other open-source rewriters (pdfcpu, pikepdf; pikepdf rows checked against 10.17.0).

Risk classes:

- **lossless**: nothing printed changes and nothing is lost.
- **metadata-only**: only descriptive data is added or brought into line (Info, XMP, identification, intent labels).
- **alters appearance**: the printed result may differ.
- **destroys information**: something is removed that the output no longer holds. Deleting metadata counts here.

`Resolves` names checks in `rust/checks.toml` that the fix would make pass. House checks are marked "house:". A trailing `?` means
the mapping depends on how our check reads the case. "—" means no check of ours is affected.

## The catalogue

### Metadata and identification

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `xmp-rebuild` | `x4.xmp.present`, `x4.xmp.conforms`, `x4.xmp.well-formed`, `x4.xmp.no-bytes-encoding-attrs`, `x4.xmp.namespace-prefixes` | Writes a fresh document XMP packet from the old packet and the Info dictionary, then adds the target's identification. | metadata-only | Custom or extension schemas in the old packet would be lost | target level | K, T, P, V, S |
| `xmp-repair-wellformed` | `x4.xmp.well-formed`, `x4.xmp.no-bytes-encoding-attrs` | Repairs an unparseable packet or drops deprecated attributes; drops the packet if it cannot be repaired. | metadata-only | The packet cannot be parsed and dropping it is not allowed | drop-if-unfixable | P, T, R |
| `xmp-namespace-prefixes` | `x4.xmp.namespace-prefixes` | Rewrites namespace declarations to the prefixes the standard requires. | metadata-only | — | — | P |
| `xmp-extension-schemas` | — | Adds or repairs the extension schema descriptions a PDF/A packet needs. | metadata-only | — | — | P |
| `xmp-prune-noncompliant` | `x4.xmp.conforms`, `x4.info.unlisted-not-in-predefined-schema`, `x4.identity.pdfxid-only-version` | Deletes XMP properties and schemas the target level does not allow, in the document packet and in per-object packets. | destroys information | The property holds data the user asked to keep | target standard | K, T, S |
| `xmp-manifest-remove` | — | Strips the edit-history manifest from XMP. | destroys information | — | — | T |
| `xmp-uncompress` | `x4.xmp.unfiltered` | Rewrites metadata streams without filters. | lossless | — | — | K, T |
| `xmp-ids-add` | `x4.xmp.document-id`, `x4.xmp.version-id`, `x4.xmp.rendition-class`, `x4.xmp.document-id-uuid` | Adds a document ID, version ID and rendition class to XMP. | metadata-only | An ID exists but is malformed (replacing it breaks links from other files) | — | P, K |
| `trailer-id-add` | `x4.trailer.id` | Generates the file identifier pair in the trailer. | metadata-only | — | — | P, Q, M, R |
| `info-xmp-sync` | `x4.info.xmp-analogue-present`, `x4.info.xmp-analogue-equal`, `x4.info.author-single-creator`, `x4.xmp.agent-names`? | Copies values between Info and XMP so each Info entry has an equal XMP twin. | metadata-only | The two disagree and no direction is given | direction | K, T, P, V, R |
| `info-legacy-keys-remove` | `x4.identity.no-info-conformance` | Removes Info keys the level forbids or deprecates, or moves them into XMP. | destroys information | — | target level | K, P |
| `info-to-xmp-migrate` | — | For PDF 2.0 levels, moves Info values into XMP and drops the Info dictionary. | metadata-only | — | target level | K |
| `dates-fill` | `x4.xmp.create-date`, `x4.xmp.modify-date`, `x4.xmp.metadata-date` | Writes the current time into absent date fields; drops a date that cannot be parsed. | metadata-only | A creation date is invented for a file that had one in another form | which dates | K, T, P |
| `title-set` | `x4.xmp.title` | Writes a title when none exists, from the file name or a given text. | metadata-only | — | source (file name or text) | K, T, P |
| `set-document-info` | `x4.xmp.title` | Sets, edits or replaces words in Info keys, optionally only where empty. | metadata-only | The key is unknown | key, value, only-if-empty | K, P, A, B, R |
| `info-text-encoding` | `x4.info.pdfx-version-pdfdoc` | Re-encodes text strings to the encoding the version needs (PDFDoc, UTF-16, UTF-8). | metadata-only | A character has no form in the target encoding | encoding | P |
| `xmp-pdf-version-sync` | — | Rewrites the PDF version recorded in XMP to the version the file is saved as. pikepdf does this on every save by default. | metadata-only | — | on or off | R |
| `editor-stamp` | `x4.xmp.modify-date`?, `x4.xmp.metadata-date`? | Writes the producer and the modification and metadata dates whenever metadata is edited. pikepdf does this by default; it is a side effect, not a requested fix. | metadata-only | — | on or off | R, K, T, P |
| `trapped-set` | `x4.trapped.present`, `x4.trapped.true-or-false` | Sets the trapped state to a chosen True or False where it is absent or Unknown. | metadata-only | The chosen value is False and TrapNet annotations exist (`x4.trapped.trapnet-implies-true`); nobody knows whether the job was trapped | value | K, T, P, G |
| `pdfx-identify` | `x4.identity.version-present`, `x4.identity.version-value`, `x4.info.pdfx-version-pdfdoc`, `x5.identification-value` | Writes the PDF/X version in Info and XMP for the target level. | metadata-only | The file does not yet pass the level (pdfToolbox writes it only after the other fixes) | level | K, T, P, G |
| `conformance-claim-remove` | — | Strips one family's identification (A, X, E, UA, VT) so the file stops claiming a standard it fails. | destroys information | — | family | K, T, P, V |
| `conformance-id-add` | — | Adds a PDF/A or PDF/UA identification to existing XMP. | metadata-only | Any other violation remains (veraPDF's rule) | part, conformance | V, T, P |
| `metadata-strip` | — | Deletes Info, XMP or page labels for privacy. | destroys information | — | which parts | Q, B, R |
| `certified-record-remove` | — | Deletes a stale Enfocus Certified PDF record. | destroys information | — | — | P |
| `opening-view-set` | — | Sets page layout, page mode, window flags, binding direction and title display. | metadata-only | — | layout, mode, window flags, binding | K, T, P |

### Output intent and colour

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `output-intent-add` | `x4.output-intent.present`, `x4.output-intent.identifier-present`, `x4.output-intent.identifier-registered`, `x4.output-intent.registry-name-color-org`, `x4.output-intent.identifier-in-registry`, `x4.output-intent.registry-name-url`, `x4.output-intent.dest-profile-present`, `x4.output-intent.output-condition`, house: `output-condition` | Embeds an output intent with a given profile, condition identifier, registry and description, or keeps a usable one already there. | metadata-only | No profile is given; the profile's class or channel count does not fit the target | profile, condition identifier, registry, description, keep-existing | K, T, P, G, S, A |
| `output-intent-replace` | `x4.output-intent.channels`, `x4.output-intent.profile-class-prtr`, `x4.output-intent.profile-version`, `x4.colour.device-space-matches-intent`? | Swaps an intent that does not fit the colour in use, or a profile of a disallowed version, for another. | alters appearance | The new condition is a guess, not the job's real press | profile, identifier | K, P |
| `output-intent-relabel` | `x4.output-intent.identifier-text-string`, `x4.output-intent.output-condition` | Rewrites the identifier, condition and info entries as proper text strings without touching the profile. | metadata-only | — | identifier, condition, registry | K, P |
| `output-intent-prune` | `x4.output-intent.unique` | Keeps a single print intent and deletes the rest. | destroys information | It is unclear which intent is the print one | which to keep | K |
| `output-intent-remove` | — | Deletes all document intents, or page-level ones. | destroys information | — | scope (document, page) | T, P, K |
| `output-intent-to-reference` | `x4p.output-intent.dest-profile-ref`, `x4p.output-intent.no-dest-profile`, `x4p.ref.profile-name`, `x4p.ref.urls`, `x4p.ref.checksum`, `x4p.ref.profile-cs`, `x4p.ref.icc-version` | Replaces the embedded profile with an external reference (name, URLs, checksum, colour space, version). | destroys information | The profile is not published at the URL given (Kura makes this only for n-colour X-5n) | profile URL | K |
| `output-intent-profile-strip` | `x4p.output-intent.no-dest-profile` | Removes the embedded profile from an intent that must reference instead. | destroys information | No reference is written in its place | — | K |
| `icc-profile-replace` | `x4.icc.profile-version`, house: `icc-profile` | Replaces a profile of a rejected class or version by an acceptable one with the same channel count, or rewrites it as version 2. | alters appearance | The profile cannot be converted | target version | K, P |
| `icc-channel-count-fix` | house: `icc-profile` | Corrects the declared channel count of an ICC stream to match the profile. | lossless | — | — | K |
| `icc-intent-duplicate-to-device` | `x4.icc.cmyk-not-intent-profile` | Replaces ICC spaces identical to the intent's profile by the plain device space. | lossless | The profiles are not byte-identical | — | K, T |
| `icc-tag` | — | Adds or removes ICC tags on objects, so device colour is calibrated or calibrated colour becomes device colour. | alters appearance | — | profile, object kind | K, P |
| `default-colour-spaces` | `x4.colour.device-space-matches-intent`? | Installs or replaces default Gray, RGB and CMYK spaces tied to profiles. | alters appearance | No suitable profile is given | space, profile | K, P |
| `convert-colours` | `x4.colour.device-space-matches-intent`, `x5.n-no-devicergb`, house: `colour-space`, `rgb-only`, `gray-only` | Converts the colour of content, images, shadings, patterns and indexed tables to a destination through a colour engine, re-encoding lossy images. | alters appearance | The destination profile is missing; shadings with per-vertex colour cannot be converted; an image cannot be decoded | destination, rendering intent, object kinds, preserve black, black point compensation, keep spots, grey to black | K, T, P, G, A, B |
| `device-link-convert` | house: `colour-space`, `tac` | Re-separates already separated colour (CMYK to CMYK, RGB to CMYK) through a DeviceLink profile, optionally limiting total ink. | alters appearance | The link does not match the source space | link profile, colour spaces, objects, ink limit | T, P, K |
| `spot-to-process` | house: `spot-ink` | Replaces spot colours by their alternate, or by grey. | alters appearance | — | which spots, overprint handling | P, T |
| `separation-alternate-fix` | `x4.spot.alternate-space-restricted` | Converts a spot's alternate space (for example RGB) to CMYK or Lab. | alters appearance | — | destination | K, P |
| `spot-unify-definitions` | `x4.spot.consistent-definition`, house: `spot-definition` | Gives every use of a spot name one alternate and tint transform, chosen by rule or swatch library. | alters appearance | No rule decides which definition is right | which wins, swatch library | K, T, P, S |
| `spot-names-merge` | `x4.spot.confusing-names`?, house: `spot-ink` | Renames spots by a map or a naming system, removes suffixes, and merges names that look the same. | alters appearance | Two names differ in appearance | map, naming system | K, T, P |
| `colour-map` | house: `spot-ink`? | Replaces one colour by another within a tolerance, to process or to a named spot, or snaps to the nearest library colour. | alters appearance | The colour text does not parse; nothing is near enough | from, to, tolerance, object kinds, library | K, T, P |
| `devicen-colorants-complete` | `x4.spot.colorants-entry`, `x4.spot.colorants-match-devicen` | Adds missing entries to the Colorants dictionary of a multi-ink space. | lossless | — | — | K, P |
| `devicen-rebuild` | `x4.spot.process-colorspace`, `x4.spot.printing-order-consistent`, `x4.spot.solidities-consistent` | Rebuilds an invalid DeviceN space (process dictionary, order, solidities), or turns it into DeviceN without NChannel data. | alters appearance | — | — | P |
| `devicen-attributes-clean` | `x4.spot.no-dotgain`, `x5.n-devicen-no-process` | Deletes DeviceN attributes the level forbids. | destroys information | — | — | K, P |
| `nchannel-to-devicen` | `x5.n-devicen-subtype` | Drops the NChannel subtype so the space is plain DeviceN. | lossless | — | — | K, T, P |
| `devicen-reduce` | house: `colour-space` | Replaces a DeviceN space with too many inks by calibrated CMYK. | alters appearance | The space is damaged | — | K |
| `devicen-split` | — | Splits multi-ink values into process plus spot. | alters appearance | — | process target | K |
| `spot-measurements-embed` | — | Attaches measured colour data (CxF) for spots. | lossless | The source is unreadable | source | K |
| `registration-to-black` | house: `tac`? | Maps registration colour in artwork to black only. | alters appearance | — | scope | K, T |
| `rich-black-to-black` | house: `rich-black`, `k-only-text` | Replaces rich, RGB or near-grey black with pure K, and grey with a K tint. | alters appearance | — | thresholds, object kinds | P, K |
| `tone-curve` | — | Applies a dot-gain or tone curve to CMYK values and images. | alters appearance | The curve does not parse | curve, colour classes | K, T |
| `transfer-functions-apply` | `x4.extgstate.no-tr`, `x4.extgstate.tr2-default` | Burns transfer curves into colours and images, or simply deletes them. | alters appearance | — | apply or delete | K, T, P, G |
| `rendering-intent-fix` | `x4.intent.known-values` | Sets a valid rendering intent or deletes an invalid one. | alters appearance | — | intent | K, T, P |
| `overprint-mode-reset` | `x4.icc.overprint-mode-zero` | Resets overprint mode where overprinted four-channel ICC colour makes it ill defined. | alters appearance | — | — | K |
| `black-point-compensation-remove` | — | Deletes the PDF 2.0 black point compensation flag from graphics states. | destroys information | — | — | T, P, K |
| `shading-tag` | — | Wraps uncalibrated shading colour spaces in a matching ICC space. | lossless | — | — | K |
| `colour-space-collision-fix` | `x4.spot.consistent-definition` | Renames or reconciles colorants with one name and conflicting definitions. | alters appearance | — | — | S |

### Fonts

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `font-embed` | `x4.font.embedded`, house: `font-embedded` | Finds the same font on the system or in a folder and embeds it, whole or subset. | lossless | The font is not found; its licence forbids embedding | font folders, subset or whole, visible text only | T, P, G, S |
| `font-substitute` | `x4.font.embedded`, `x4.font.glyphs-present`, house: `font-embedded` | Embeds a similar font chosen by name, metrics or the characters used, keeping widths. | alters appearance | No font is similar enough; script not covered | font folders, minimum similarity, target font | K, T, P, S |
| `font-reembed-complete` | — | Re-embeds a subset font completely, or subsets a complete one. | lossless | The original font is not available | complete or subset | P, G, M |
| `font-unembed` | — | Removes an embedded program for a standard font. | alters appearance | Always for PDF/X (breaks `x4.font.embedded`) | — | P, B |
| `font-page-rasterise` | `x4.font.embedded` | Renders a page whose font can be neither embedded nor substituted, keeping the text invisible on top. | destroys information | No renderer | resolution | K |
| `text-to-outlines` | `x4.font.embedded`, `x4.font.glyphs-present`, `x4.font.no-notdef`, `x4.font.widths-consistent`, house: `font-embedded`, `missing-glyph`, `font-type3` | Turns text into vector paths. | destroys information | Text must stay searchable (UA levels) | scope | K, T, P, M, B |
| `invisible-text-copy` | — | Adds an invisible copy of text that was outlined or rasterised, for search. | lossless | — | scope | K, T |
| `notdef-to-space` | `x4.font.no-notdef` | Replaces text that shows .notdef by a space glyph of the same width. | alters appearance | The .notdef glyph has visible ink | only if empty | T, P |
| `missing-glyph-remove` | `x4.font.glyphs-present`, house: `missing-glyph` | Deletes characters whose glyphs the font lacks or whose encoding is invalid. | destroys information | — | condition | K, T |
| `space-glyph-add` | `x4.font.glyphs-present`? | Adds a missing space glyph to an embedded subset. | lossless | — | — | T |
| `widths-sync` | `x4.font.widths-consistent` | Rewrites the Widths array from the font program, Type 3 widths from the glyph drawings, and fake-bold widths. | alters appearance | — | — | K, T, P |
| `truetype-encoding-fix` | `x4.font.truetype-nonsymbolic-encoding`, `x4.font.truetype-nonsymbolic-no-differences`, `x4.font.truetype-symbolic-no-encoding` | Removes Differences or the Encoding entry from TrueType fonts, or sets a standard encoding, and recomputes the symbolic flag. | alters appearance | The glyph mapping would change | — | K, T |
| `truetype-to-cff` | `x4.font.truetype-nonsymbolic-no-differences`, `x4.font.truetype-symbolic-one-cmap` | Converts a TrueType font to a CFF-based one. | lossless | — | — | T, B |
| `truetype-cmap-fix` | `x4.font.truetype-symbolic-one-cmap` | Prunes extra cmap subtables from a symbolic TrueType font, or adds a Unicode one. | lossless | — | — | K, T |
| `charset-cidset-fix` | — | Rebuilds CharSet and CIDSet from the subset, or deletes them when they cannot be right. | lossless | — | rebuild or delete | K, T |
| `cid-font-fix` | — | Makes CIDSystemInfo, CIDToGIDMap and CMap consistent, inserts a missing CMap, or falls back to identity. | alters appearance | Identity would change the text mapping | — | K, T |
| `font-dict-repair` | `x4.pdf-reference.conforms`? | Adds missing required font entries, wraps a bare CID font in a composite one, or substitutes a standard font for a non-font. | lossless | — | — | K |
| `font-program-repair` | — | Trims trailing data and fixes section lengths in Type 1 programs, unwraps OpenType to bare CFF, moves width operators into Type 3 glyphs. | lossless | — | — | K, S |
| `font-names-fix` | `x4.names.utf8` | Renames fonts with invalid names, makes subset names unique, or renames by a map. | lossless | — | map | K, T |
| `font-metadata-add` | — | Adds font metadata. | lossless | — | — | P |
| `text-render-mode-set` | — | Rewrites text render modes, for example to show hidden text. | alters appearance | — | mode, scope | K |

### Images

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `image-downsample` | house: `image-dpi-max` | Resamples images to a target resolution at the size they are drawn. | alters appearance | The image cannot be decoded | image kind, target ppi, threshold ppi, method | K, T, P, G, M, Q, B, A |
| `image-upsample` | house: `image-dpi` | Resamples images up to a floor. | alters appearance | Always, by our rules: it passes the check without adding detail | target ppi | K |
| `image-recompress` | — | Re-encodes images with another filter or quality (JPEG, Flate, CCITT, JBIG2, JPEG 2000). | alters appearance | The result would be larger | format, quality, min size | K, T, P, G, M, Q, B |
| `image-sharpen` | — | Sharpens pixels after resampling. | alters appearance | — | radius, amount | K |
| `jpx-transcode` | `x4.image.jpx-valid`, `x4.jpx.baseline`, `x4.jpx.channels`, `x4.jpx.one-colour-space`, `x4.jpx.no-ciejab`, `x4.jpx.cmyk-as-device`, `x4.jpx.bit-depth`, `x4.jpx.uniform-depth` | Re-encodes JPEG 2000 losslessly, or as JPEG for old PDF versions. | alters appearance | The data cannot be decoded | target format, quality | K, T, P |
| `jpx-colour-disambiguate` | `x4.jpx.one-colour-space` | Resolves approximate JPEG 2000 colour specifications to one. | lossless | — | — | T |
| `jbig2-reencode` | `x4.image.jbig2-valid` | Re-encodes JBIG2 data losslessly. | lossless | No renderer | — | K |
| `jpeg-repair` | `x4.image.dct-valid` | Decodes what can be recovered from damaged JPEG and re-encodes it; leaves undecodable images and reports them. | destroys information | The image cannot be decoded at all | — | K |
| `image-header-fix` | `x4.image.dct-valid`? | Corrects declared width, height or colour space to match the JPEG data. | lossless | — | — | K, S |
| `image-depth-reduce` | — | Reduces 16-bit images to 8 bits, or re-encodes an unsupported depth. | alters appearance | — | — | K, T |
| `interpolate-clear` | — | Clears or sets the image smoothing flag. | alters appearance | — | value | K, T, P |
| `alternates-remove` | `x4.image.no-default-for-printing` | Deletes alternate images, keeping the default, highest or lowest. | destroys information | — | which to keep | K, T, P |
| `opi-remove` | `x4.xobject.no-opi` | Deletes OPI dictionaries; the low-resolution image stays. | destroys information | — | OPI version | K, T, P |
| `soft-mask-simplify` | — | Drops opaque soft masks and turns two-level ones into stencil masks. | lossless | — | — | K |
| `colour-key-mask-drop` | — | Drops a colour-key mask that no longer matches after colour conversion. | alters appearance | — | — | K |
| `image-fragments-merge` | — | Merges an image stored as strips into one. | lossless | — | scope | K, T |
| `inline-image-fix` | `stream.filter`? | Rewrites inline image syntax and filters, or moves inline images out to XObjects. | lossless | — | min size | K, P, Q, R |
| `image-rotate` | — | Rotates an image object. | alters appearance | — | angle | P |
| `image-replace` | — | Extracts or replaces images by hand. | alters appearance | — | — | R |
| `deskew` | — | Straightens scanned pages. | alters appearance | — | — | B |

### Transparency

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `transparency-flatten` | `x4.transparency.blend-modes`, `x4.transparency.group-cs-restricted` (by removal) | Computes composites and writes opaque objects, as vectors where possible, by region, then by rasterising each transparent object. | alters appearance | A case is too complex; no renderer; the level allows transparency | raster resolution, line-art and gradient resolution, text to outlines, overprint handling, blend strategy | K, T, P, G, S, A |
| `page-rasterise` | — (replaces the content the checks read) | Renders whole pages to images, optionally with invisible text; also a last fallback when a converted file still fails. | destroys information | No renderer; not opted in | resolution, colour space, compression, keep text | K, T, P, B |
| `blend-space-set` | `x4.transparency.group-cs-restricted`, `x5.n-transparency-group-cs`, house: `blend-space` | Adds a page group where transparency is used without one, and sets its blending space from the intent or a choice. | alters appearance | — | space, only if missing | K, T, P |
| `blend-mode-normalise` | `x4.transparency.blend-modes` | Replaces unknown or non-standard blend modes with Normal. | alters appearance | Not opted in to visible change | mode | K, P, S |
| `alpha-clamp` | — | Sets out-of-range opacity to fully opaque. | alters appearance | — | — | K |
| `jpx-alpha-composite` | — | Composites JPEG 2000 alpha onto white. | alters appearance | — | — | K |
| `transparency-groups-remove` | — | Deletes page groups where nothing blends, and groups on forms where the level forbids them. | lossless | Something on the page blends | scope | K, T |
| `overprint-to-transparency` | — | Replaces an overprint situation that flattening would lose with a blend-mode group. | alters appearance | — | blend mode, fill or stroke | T |

### Page geometry and boxes

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `trimbox-set` | `x4.boxes.trim-or-art` | Adds a TrimBox from the CropBox or MediaBox, or from another box with offsets. | lossless | The real trim is unknown: copying the CropBox is a claim, not a repair | box, reference box, offsets, only if missing | K, T, P, G, R |
| `boxes-from-marks` | `x4.boxes.trim-or-art`, house: `geometry`, `bleed` | Derives TrimBox and BleedBox from crop marks in the artwork. | lossless | No marks are found | tolerance, mark colour | T, P |
| `bleedbox-set` | `x4.boxes.within-bleed` | Adds or clamps a BleedBox, derived from the trim with an offset. | lossless | — | offset | K, G |
| `boxes-nest` | `x4.boxes.within-bleed`, `x4.boxes.within-crop`, `x4.boxes.within-media` | Adjusts boxes as little as possible so they nest, and clamps them to size limits. | alters appearance | — | — | K, T, A |
| `artbox-remove` | `x4.boxes.not-both`, `x4.boxes.prefer-trim` | Deletes the ArtBox. | destroys information | The ArtBox is the only box describing the finished page | — | T |
| `mediabox-set` | `page.media-box` | Sets a MediaBox where none is usable. | alters appearance | — | — | K |
| `media-origin-move` | — | Moves the MediaBox's lower left corner to the origin, translating content. | lossless | — | — | K, T, P |
| `user-unit-apply` | — | Scales a page by its UserUnit for versions that lack it. | lossless | — | — | K |
| `bleed-generate` | house: `artwork-bleed`, `bleed` | Extends artwork beyond the trim by mirroring or stretching edges, and enlarges the media. | alters appearance | No TrimBox | width, method, scope | K, T, P |
| `bleed-around-shape` | — | Grows artwork outward around a die-line contour. | alters appearance | No die-line is found | width, resolution, die-line selector | K, P |
| `crop-to-content` | — | Sets the CropBox to the visible content. | alters appearance | No renderer | box | K, T |
| `outside-content-remove` | house: `outside-bleed`, `outside-media` | Deletes objects wholly outside a box, or clips content to it. | destroys information | — | box | K, T, P, A |
| `page-scale` | house: `geometry`, `page-size` | Scales page and content to a size, or sets one size for all pages. | alters appearance | — | width, height, fit mode, only if larger | K, T, P, B |
| `page-rotate` | house: `page-rotate` | Rotates pages, or pushes the Rotate key into the content. | lossless | Angle not a multiple of 90 | angle, orientation filter | K, T, P, G, Q, R |
| `page-mirror` | — | Mirrors pages. | alters appearance | — | axis | K, T |
| `pages-add` | house: `page-count` | Adds blank pages to meet a page-count rule. | lossless | — | count, position | P, T |
| `empty-pages-remove` | house: `page-count` | Deletes blank or visually empty pages. | destroys information | — | box to test | P, T |
| `page-tree-rebuild` | — | Rebalances or rebuilds a too-wide or damaged page tree. | lossless | — | — | K |

### Structure, syntax and repair

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `xref-rebuild` | `file.xref`, `file.startxref`, `file.trailer`, `object.damaged` | Rebuilds the cross-reference and object table by scanning the file, and writes it out clean. | lossless | The file is beyond repair; the scan had to guess | suppress recovery, parser limits | K, Q, M, S, B, R |
| `rewrite-clean` | `file.header-offset`, `file.eof`, `syntax.hex-digits`, `syntax.octal-escape`, `syntax.name-escape`, `syntax.duplicate-key`, `stream.eol` | Re-serialises every object through a writer, so syntax faults vanish. | lossless | A duplicate key or bad escape forced the parser to choose a value | object streams, newline before endstream, keep metadata | Q, M, R, P |
| `content-normalise` | `x4.content.known-operators`?, `x4.content.no-bx-ex`?, `content.compatibility`? | Parses and rewrites content streams, fixing bad operand counts and dropping invalid operators. | alters appearance | A stream cannot be decoded | — | K, Q, M |
| `content-undecodable-empty` | `content.damaged` | Replaces a content stream that cannot be decoded with an empty one. | destroys information | — | — | K |
| `missing-resource-fix` | `x4.complete.resources-inside`, house: `xobject-missing` | Drops operators that name missing resources, or adds an empty placeholder. | alters appearance | — | scope | K |
| `resources-tidy` | — | Lists inherited resources explicitly and removes unused ones. | lossless | — | keep unreferenced | K, Q, P, R |
| `inherited-attributes-push` | — | Copies attributes a page inherits from the page tree (boxes, resources, rotation) onto each page. pikepdf does it on open by default. | lossless | — | on or off | Q, R |
| `content-coalesce` | — | Joins a page's array of content streams into one stream. | lossless | A stream cannot be decoded | — | Q, R |
| `resource-cycle-cut` | — | Breaks a form or pattern that draws itself. | alters appearance | — | — | K |
| `required-entries-add` | `x4.pdf-reference.conforms`? | Adds missing required entries to forms, patterns and shadings, and removes a bad bounding box. | lossless | — | — | K |
| `postscript-remove` | `x4.xobject.no-postscript`, `x4.xobject.no-subtype2-ps` | Deletes PostScript XObjects and fragments. | destroys information | — | — | K, T, P |
| `lzw-to-flate` | `x4.filter.no-lzw` | Recompresses LZW streams with Flate. | lossless | — | — | K, T |
| `filter-replace` | `x4.filter.standard-only`, `x4.filter.no-crypt`, `stream.filter` | Decodes streams with a disallowed filter and stores them with a standard one; empties streams with an unknown filter. | lossless | The filter is unknown, so the data would be lost | — | K, P |
| `filter-names-expand` | `stream.filter`? | Expands abbreviated filter names outside inline images. | lossless | — | — | K |
| `external-streams-remove` | `x4.stream.no-external-f` | Cuts stream references to external files. | destroys information | The external data could be embedded instead | — | K, P |
| `external-refs-remove` | `x4.xobject.no-ref`, `x4.filespec.embedded` | Deletes external references (reference XObjects, remote go-to links), or completes them. | destroys information | The level allows references (X-5g) | — | K, T |
| `names-to-utf8` | `x4.names.utf8`, `x4.names.hex-escaped`, `x4.names.other-recommended` | Re-encodes names (fonts, colorants, structure) as escaped UTF-8. | lossless | A spot name changes and plates are named by it | — | K, T |
| `limits-clamp` | `x4.limits.lexical`, `x4.limits.graphics` | Clamps numbers, strings and nesting to the version's limits. | alters appearance | Pathological nesting: rejected outright | target version | K |
| `object-streams-remove` | — | Writes a classic cross-reference without object streams. | lossless | — | — | T, P, Q |
| `pdf-version-set` | — | Saves as a chosen PDF version, removing what it lacks (layers, 16-bit images, JPEG 2000, transparency). | alters appearance | — | version | K, T, P, Q, A |
| `pdf2-features-remove` | — | Strips PDF 2.0 features (associated files, black point compensation, DPart, viewer enforcement, mixing hints, spectral data, requirements). | destroys information | — | feature list | P, K |
| `viewer-areas-remove` | `x4.viewer-prefs.areas` | Deletes the view and print area and clip entries from viewer preferences. | destroys information | — | — | K, T |
| `catalog-keys-remove` | — | Deletes catalog entries the level forbids. | destroys information | — | — | K |
| `private-data-remove` | — | Deletes application private data (PieceInfo). | destroys information | — | — | K, T |
| `job-tickets-remove` | — | Deletes embedded job tickets. | destroys information | — | — | T |
| `file-encoding-set` | — | Re-serialises as ASCII or binary, or in an inspectable form. | lossless | — | ascii or binary | P, Q |

### Annotations, forms and actions

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `annotations-move-out` | `x4.annot.outside-bleed`, `x4.annot.printermark-outside-trim` | Moves annotations out of the trim or bleed, sparing TrapNet and printer's marks. | alters appearance | — | box, types spared | T, P |
| `annotations-remove` | `x4.annot.outside-bleed`, `x4.annot.printermark-outside-trim` | Deletes annotations by type, including ones whose appearance font cannot be embedded. | destroys information | — | types | K, T, P, S, R, B |
| `annotations-flatten` | `x4.annot.outside-bleed` | Draws an annotation's appearance into the page content, or turns it into a stamp. | alters appearance | No appearance exists | types | K, T, P, Q, S, B, R |
| `annotation-flags-set` | — | Sets print and hidden flags as the standard requires. | alters appearance | — | flags, types | K, T, P |
| `appearances-generate` | — | Builds missing appearance streams and states for annotations and fields. | lossless | — | — | K, T, Q, M, R |
| `appearances-trim` | — | Keeps only the normal appearance, dropping rollover and down states. | destroys information | — | — | K |
| `annotation-colour-remove` | — | Deletes annotation colour entries under a non-RGB intent. | alters appearance | — | — | K |
| `trapnet-fauxing-clear` | `x4.trapnet.font-fauxing-empty` | Removes the font substitution list from TrapNet annotations. | destroys information | — | — | K |
| `forms-remove` | `x4.acroform.no-xfa` | Deletes the AcroForm and its fields. | destroys information | — | — | K, T, P, Q, R |
| `forms-flatten` | — | Burns field appearances into the page and removes the fields. | alters appearance | No appearance exists | — | T, P, B |
| `need-appearances-clear` | — | Clears the flag asking viewers to build appearances. | lossless | — | — | K |
| `xfa-remove` | `x4.acroform.no-xfa` | Deletes XFA data. | destroys information | — | — | K, S |
| `actions-remove` | `x4.actions.absent`, `x4.javascript.absent`, `x4.names.no-alternate-presentations`, `x4.page.no-pres-steps` | Deletes actions, scripts, open actions, additional actions, form submit and reset actions, and presentations. | destroys information | — | action kinds | K, T, P, S |
| `javascript-remove` | `x4.javascript.absent` | Deletes JavaScript only. | destroys information | — | — | T, P, K |
| `hyperlinks-remove` | `x4.actions.absent`? | Deletes external links, or links that no longer work. | destroys information | — | — | P, T |
| `navigation-remove` | — | Deletes thumbnails, bookmarks, article threads or unused destinations. | destroys information | — | which | P, T, R |
| `outline-repair` | — | Rebuilds broken bookmark parent and sibling links. | lossless | — | — | K |
| `links-from-text` | — | Adds link annotations for URLs found in the text. | lossless | — | — | K |
| `rich-media-remove` | — | Deletes multimedia and 3D content. | destroys information | — | — | K, S |

### Optional content

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `oc-config-repair` | `x4.oc.config-name-present`, `x4.oc.config-name-unique`, `x4.oc.order-complete`, `x4.oc.no-as` | Adds a missing default configuration, names configurations uniquely, completes the Order, removes AS. | lossless | — | — | K, T, P |
| `layers-flatten` | — | Keeps visible content, optionally drops hidden content, and removes the layer structure. | destroys information | Hidden layers hold content someone may need (a language, a varnish) | keep hidden, which layers | K, T, P, S |
| `empty-layers-remove` | — | Deletes layers with no content. | lossless | — | — | P |
| `layer-state-set` | — | Edits layer names, default state, print, view and export usage, matched by pattern. | alters appearance | — | match, state, usage, name | K, P, B |
| `layer-views-add` | — | Adds a configuration that shows or hides named layers. | lossless | No layers | name, show, hide | K, T |
| `objects-to-layer` | — | Moves selected objects onto a named layer or an ISO 19593-1 processing step. | lossless | Objects inside patterns, Type 3 glyphs or appearances | layer, object selector, processing step | K, T, P |
| `oc-membership-remove` | — | Removes layer membership from annotations. | alters appearance | — | — | K |

### Security and signatures

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `decrypt` | `x4.encrypt.absent`, `file.encrypted`, `x4.filter.no-crypt` | Decrypts with an empty or given password and writes the file clean; the protection itself is gone. | lossless | No password; a non-standard security handler | password | K, Q, M, R |
| `restrictions-remove` | `x4.perms.ur-only` | Removes permission restrictions. | destroys information | — | — | Q, B |
| `signatures-remove` | — | Strips signatures that a change would invalidate, and warns. | destroys information | Not told to drop signatures (Hyper Compress skips signed files) | — | K, S, B, M |
| `signature-fields-remove` | — | Deletes unsigned signature fields. | destroys information | — | — | K |
| `sign-output` | — | Signs the output after conversion. | lossless | — | certificate | K |
| `certify-output` | — | Embeds a certificate entry proving the result. | lossless | — | certificate | K |

### Size and optimisation

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `streams-compress` | — | Applies Flate to uncompressed streams except XMP, or recompresses at a level. | lossless | — | level, recompress flate | T, P, Q, M, R |
| `object-streams-write` | — | Packs objects into object streams. | lossless | The level forbids object streams | mode | Q, B |
| `linearise` | — | Writes the file for fast web view. | lossless | — | — | K, T, Q, G, B |
| `objects-dedupe` | — | Merges duplicate objects, fonts and images; compacts the cross-reference. | lossless | — | level | M, R, G, B |
| `thumbnails-remove` | — | Deletes embedded page thumbnails. | lossless | — | — | K, T, P |
| `font-programs-optimise` | — | Converts Type 1 to CFF, merges duplicate font programs, compresses font streams, drops embedded standard fonts. | alters appearance | Dropping embedded fonts breaks `x4.font.embedded` | font options | B, M |
| `colour-data-simplify` | — | Rewrites colour spaces and replaces large embedded profiles with simpler ones. | alters appearance | — | preset | B |
| `brotli-compress` | — | Compresses streams with Brotli (PDF 2.0 only). | lossless | Any PDF/X-4 target (breaks `stream.filter`) | — | B |
| `optimise-preset` | — | Applies a named bundle of image, font and compression settings. | alters appearance | The output would be larger; a page changes visibly (Hyper Compress rolls back) | preset, target size | G, B, A |

### Accessibility and tagging

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `markinfo-set` | — | Sets the tagged flag. | metadata-only | — | — | K, T |
| `structure-synthesise` | — | Creates a tag tree from page content, or replaces an unusable one. | destroys information | The old tree would be lost | — | K |
| `structure-repair` | — | Repairs tags, role maps, element types, IDs and note IDs. | lossless | — | — | K, T |
| `structure-remove` | — | Drops the tag tree or marked content. | destroys information | — | — | S, M, Q, P |
| `artifacts-mark` | — | Marks untagged content, running heads and footers as artifacts. | lossless | — | — | K, T |
| `headings-lists-fix` | — | Merges split headings, sets list labels and numbering. | alters appearance | — | — | K, T |
| `empty-elements-remove` | — | Removes empty block elements. | destroys information | — | — | T |
| `language-set` | — | Sets, normalises or copies the document language, and fixes language on list labels and marked content. | metadata-only | No value is given or found | language | K, T |
| `link-alt-text` | — | Writes alternate text for links and points links at their structure elements. | metadata-only | — | — | K, T |
| `tab-order-set` | — | Sets tab order to follow the structure. | lossless | — | — | T |
| `display-title-set` | — | Shows the document title in the window. | metadata-only | — | — | K, T |
| `outline-from-headings` | — | Builds bookmarks from tagged headings or a list. | lossless | No tagged headings | source | K, T |
| `tounicode-build` | — | Builds or completes Unicode mappings and scrubs forbidden code points. | lossless | The mapping cannot be derived | — | K |
| `actual-text-fix` | — | Adds replacement text to symbol and private-use characters, and strips private-use characters from it. | lossless | — | — | K |
| `ocr-text-layer` | — | Recognises scanned text and lays it invisibly over the page. | lossless | — | language | K |
| `pdfua-identify` | — | Adds the PDF/UA identification and sets MarkInfo. | metadata-only | — | — | T |

### Attachments and e-invoices

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `embedded-files-remove` | — | Deletes attached files and file-attachment annotations. | destroys information | The level allows attachments | — | K, T, S |
| `filespec-repair` | — | Adds missing file names and Unicode names to file specifications. | lossless | — | — | T, K |
| `associated-files-link` | — | Links attachments as associated files, sets relationships and MIME types, repairs the name tree. | metadata-only | — | relationship | K |
| `invoice-attach` | — | Embeds invoice XML with the name and relationship its guideline asks, and declares it in XMP. | lossless | The XML's guideline is not recognised | XML file | K |
| `source-attach` | — | Attaches the original file, or any XML file. | lossless | — | file | K |

### Other print production

| Fix | Resolves | Method | Risk | Refuses when | Parameters | Offered by |
|---|---|---|---|---|---|---|
| `halftones-remove` | `x4.extgstate.no-htp`, `x4.halftone.type-1-or-5`, `x4.halftone.no-name`, `x4.halftone.transfer-function` | Deletes or normalises halftones, names, origin and phase, adding a transfer function where one is required. | destroys information | — | which settings | K, T, P, G |
| `device-settings-remove` | — | Deletes black generation, undercolour removal, flatness and smoothness from graphics states. | alters appearance | — | settings, scope | K, T, P, G |
| `graphics-state-set` | — | Writes flatness, rendering intent, overprint mode, blend modes or alpha into graphics states. | alters appearance | — | each value, scope | K, P |
| `overprint-black` | — | Sets overprint on pure black text and vectors. | alters appearance | — | object kinds | K, T, P |
| `knockout-white` | house: `white-overprint` | Forces knockout on white objects. | alters appearance | — | object kinds | K, T, P |
| `overprint-set` | house: `overprint`?, `white-overprint` | Sets overprint, knockout or overprint mode on selected objects. | alters appearance | — | what, on or off, selector | T, P, G |
| `overprint-simulate` | house: `overprint` | Bakes overprint into plain colour. | alters appearance | Images and forms need other handling | — | K |
| `line-width-increase` | house: `hairline` | Raises strokes thinner than a floor, or turns thin filled shapes into strokes. | alters appearance | — | threshold, new width | K, T, P, A |
| `dash-phase-fix` | — | Repairs negative dash phases and invalid dash arrays. | alters appearance | — | — | K, P |
| `invisible-content-remove` | — | Deletes invisible text, line art or objects. | destroys information | — | object kinds | P |
| `text-size-change` | house: `text-size`? | Changes point size, line weight or scales a selection. | alters appearance | — | size, scope | P |
| `bring-to-front` | — | Moves text or vectors above images. | alters appearance | — | object selector | K, T |
| `curves-smooth` | — | Fits curves to polylines. | alters appearance | — | — | K |
| `white-plate` | — | Builds a white underprint spot plate from objects or shapes, grown or shrunk. | alters appearance | — | spot name, source, shape, opacity | K, T |
| `printer-marks-add` | — | Draws crop and registration marks, optionally enlarging the media. | alters appearance | No TrimBox | offset, length, line width, enlarge media | K, P |
| `content-place` | — | Places text, page numbers, shapes, barcodes, another PDF page or HTML at an anchor. | alters appearance | The barcode data cannot be encoded | content, position, anchor, pages | K, T |
| `separation-info-remove` | `x4.pre-separated.absent`? | Deletes separation information from composite pages. | destroys information | — | — | K |
| `vt-structure` | — | Builds or cleans PDF/VT record structure and part metadata. | metadata-only | Ranges do not cover the pages contiguously | record ranges | K |
| `dual-conformance` | — | Converts to a print level, then converts that output to an archive level, for one file claiming both. | alters appearance | The pair of levels is not supported | both levels | K |
| `output-name-suffix` | — | Appends a level suffix to the output file name. | lossless | — | suffix | T |

## How the tools structure fixes

| Tool | Unit of fix | Selection and conditions | Ordering | Verification after fixing | Rollback and refusal | Reporting |
|---|---|---|---|---|---|---|
| Kura | Fixed passes for a target level, plus optional profile fix-ups with named parameters | Each pass is gated by the level; a profile fix-up can be tied by name to checks and then runs only on pages where they hit | Fixed pass order (structure, boxes, images, resources, colour, print, fonts, tagging, metadata, limits); profile fix-ups run first, in listed order, some folded into the passes as options | Runs its own checker on the output; a failed PDF/A result can be converted again, then rasterised, each accepted only if it re-verifies | No per-fix undo; a plan can revert to the input; destructive repairs need a visual-risk opt-in, otherwise it rejects and suggests another level | Every action is an issue: code, fixed flag, severity, pages |
| pdfToolbox | Configured fixup types in a profile; the PDF/X conversion expands to a fixed internal list | A fixup may carry an "apply to" check that selects pages or objects; the check selects, it does not trigger; variables set values at run time | Order of the fixup sets; process plans chain profiles, fixups and checks with branches on result | A fix run ends with a fresh analysis unless a fast mode skips it | Process-plan branches; the X conversion writes identification only once the file passes | Per fixup, counts of successes and failures; remaining check hits |
| PitStop | A fix toggle beside each check in a profile; Actions in ordered Action Lists | Profiles pair check and fix; Action Lists use Selection Actions (with AND, OR, NOT) and named restrictions to scope later Changes | Action List order | The usual workflow re-runs the preflight | Some problems are documented as "fix in the source"; no undo beyond the saved original | Counts per fix with page ranges; items that could not be fixed |
| Ghostscript | One regenerating write; settings, not fixes | Mode switches (PDF/X, PDF/A) force colour strategy and font embedding | Implicit in the single pass | None | A policy for PDF/A: keep and warn, drop and warn, or abort | Warnings on the console |
| veraPDF | Metadata fixes only | Runs only when the file has no other violation | n/a | Validation runs first; the fix is its last step | Refuses whenever anything else is wrong, so it never certifies a bad file | The fixed file and a list of changes |
| Hyper Compress | Size presets | Preset and target size | Internal | Validates the output | Rolls back changes that visibly alter a page; never returns a larger file; skips signed files unless told | Before and after size |
| SDKs (PDF Tools, Apryse) | A PDF/A conversion that emits events | Target level | Internal | Apryse recommends re-validating the output | The caller decides which event severities are acceptable | Events with severity (information, warning, error), Apryse with changed object numbers |
| pikepdf | Library calls on qpdf, plus options on open and save | The caller picks calls; several repairs are defaults that run on every open or save (recovery, pushing inherited attributes, compressing streams, syncing the XMP version, stamping the editor) | The caller's code | None built in; a syntax check and qpdf's warnings are there to call | Keeps a PDF/A claim through edits by default, whether or not the edit keeps the file conformant; no refusal | qpdf warnings; a flag on metadata it had to recover |

Three patterns recur. A check is used as a **selector**, not a trigger: it says where a fix applies, and the fix decides how. Writing
the **identification comes last**, once everything else passes. And the trustworthy tools **re-verify** after fixing, with the same
engine; none of them uses an independent checker. Libraries such as pikepdf are the opposite case: they repair silently on open and save,
and keep a conformance claim through an edit without checking it, so a file that passed can stop passing while still claiming to.

The pdfToolbox PDF/X-4 conversion is a useful reference set. Its 33 built-in fixups map to these rows: `actions-remove`,
`postscript-remove`, `forms-remove`, `opi-remove`, `transfer-functions-apply`, `trapped-set`, `artbox-remove`, `boxes-nest`,
`blend-space-set`, `output-intent-add`, `streams-compress`, `lzw-to-flate`, `convert-colours`, `font-embed`, `title-set`,
`trimbox-set`, `notdef-to-space`, `viewer-areas-remove`, `black-point-compensation-remove`, `output-intent-remove` (page level),
`annotations-move-out`, `pdfx-identify`, `halftones-remove`, `output-name-suffix`, `spot-unify-definitions`, `xmp-rebuild`,
`widths-sync`, `oc-config-repair`, `icc-intent-duplicate-to-device`, `xmp-prune-noncompliant`, `truetype-encoding-fix`,
`xmp-uncompress` and `names-to-utf8`. PDF/X-1a and X-3 add flattening of transparency and layers, removal of embedded files and
alternates, a PDF 1.3 save and JPEG 2000 to JPEG.

## Coverage against proof-pdf's checks

Counted from the `Resolves` column over the 223 checks marked `implemented` in `rust/checks.toml`. A check counts as covered if at
least one row names it; 8 of those are only marked `?`.

| Group | Implemented | With a known fix | Without |
|---|---|---|---|
| PDF/X-4 (`x4.*`) | 125 | 121 | 4 |
| PDF/X-4p references and file sets (`x4p.*`) | 12 | 7 | 5 |
| PDF/X-5 identification (`x5.identification-value`) | 1 | 1 | 0 |
| PDF/X-5g (`x5.g-*`) | 16 | 0 | 16 |
| PDF/X-5n (`x5.n-*`) | 15 | 4 | 11 |
| File and syntax (`file.*`, `syntax.*`, `stream.*`, `content.*`, `page.*`, `object.*`) | 20 | 16 | 4 |
| House | 34 | 29 | 5 |
| **Total** | **223** | **178** | **45** |

Covered only weakly (`?`): `x4.pdf-reference.conforms`, `x4.pre-separated.absent`, `x4.spot.confusing-names`, `x4.xmp.agent-names`,
`x4.content.known-operators`, `x4.content.no-bx-ex`, `content.compatibility` and house `text-size`.

Several covered rows are covered only by a destroying fix: outlining text, rasterising, removing annotations or actions. Several
more are covered only by a fix that makes a claim (`trapped-set`, `trimbox-set`, `output-intent-add`). Coverage says a tool can
make the check pass, not that it should.

Without any known fix, 45:

- **PDF/X-4:** `x4.output-intent.no-dest-profile-ref` (no tool turns a reference back into an embedded profile),
  `x4.trapped.trapnet-implies-true` (only by asserting True or deleting the TrapNet), `x4.trapnet.pcm-matches-intent`,
  `x4.limits.objects`.
- **PDF/X-4p:** `x4p.ref.urls-no-ef-platform`, `x4p.ref.no-colorant-table`, `x4p.fileset.checksum-matches`,
  `x4p.fileset.fields-match`, `x4p.fileset.profile-valid`. A fix would have to fetch or edit the external profile.
- **PDF/X-5g:** all 16 implemented `x5.g-*` rows. They concern other files in the set.
- **PDF/X-5n:** `x5.n-profile-iso15076`, `x5.n-trapnet-pcm`, `x5.n-trapnet-separations`, `x5.n-urls-ef-allowed`,
  `x5.n-profilecs-xclr`, `x5.n-3clr-4clr-not-rgb-cmyk`, `x5.n-colorant-table-present`, `x5.n-colorant-table-count`,
  `x5.n-colorant-table-matches-profile`, `x5.n-devicegray-needs-black`, `x5.n-devicecmyk-needs-cmyk`.
- **File and syntax:** `file.header`, `file.limit`, `content.operator`, `object.dangling-reference`. A rewrite through qpdf or
  mutool may cure the last two, but no source says so.
- **House:** `image-stretch`, `contrast`, `safe-area`, `centring`, `pdfa` (veraPDF's verdict; the PDF/A fixes in the catalogue
  would be indirect at best).

## Notes for a fix library

Ideas, not decisions.

- A fix could be a record: a **selector** over one check's findings (check id plus object or page), an **action**, named
  **parameters** with no defaults, a **risk class**, **preconditions** under which it refuses, and the checks to **re-run** after.
- Selecting by our own finding, as pdfToolbox's "apply to" does, keeps the fix tied to evidence: no finding, no change.
- Refuse rather than destroy. A fix in the destroying class runs only when asked for by name; a fix that would invent a fact
  (trim from crop, trapped state, a printing condition, upsampled detail) refuses unless the value is supplied.
- Ordering the tools reveal: structure and decryption first; boxes before bleed and before anything that clips; colour conversion
  before flattening, so blending happens in the destination space; image resampling before recompression; font embedding, then
  substitution, then outlining as the last resort; identification and XMP last, once all else passes.
- Re-verify the output with the full independent checker, not only the fixed checks. A fix that leaves a new failure is undone.
- Report every change: fix id, rule-set version, objects and pages touched, risk class, and the before and after verdicts.
- Keep the verifier unchanged. The fix library reads findings and writes a new file; it never feeds back into how a check passes.
- Version the rule set separately, with its own changelog, so a verdict names the checker version and a fixed file names the
  fix-set version.
- Ship lossless and metadata-only fixes first. They cover most of the metadata, identity, filter, name and syntax rows, and the
  owner can judge the risk of the rest one by one.
- Some fixes are claims, not repairs: `trapped-set`, `trimbox-set`, `output-intent-add`. They need a value from the user.
- Never touch a signed file without an explicit choice, as Hyper Compress does.
- Never write a PDF/X identification ourselves unless the result passes our own checks; otherwise proof-pdf would certify its
  own output.
- Kura's plans (check, fix, re-check, revert) and pdfToolbox's process plans suggest one small loop is enough; a general rule
  engine is not needed.
- Licences: ideas from Kura only; rule names and wording here are ours.
