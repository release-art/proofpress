# Note: cover checks (parked)

Not planned and not scheduled. This records what `ingram-check` 0.1.8 does for book covers and what proof-pdf would need, so
the idea is not lost. It was split out of `docs/briefs/printer-spec-gaps.md`, where covers are explicitly out of scope. The
house rules in `docs/briefs/bleed-and-suites.md` would apply to any work on it: printer numbers are parameters with no
defaults, an unconfirmable check fails, and nothing is invented.

## What ingram-check does for covers (read from the 0.1.8 source, MIT)

Nine checks on `ingram-check cover FILE -t TRIM [-b perfectbound|casewrap]`:

| Check | What it does | Worth knowing |
|---|---|---|
| font embedding | all fonts embedded | we already have this |
| page count | the cover PDF has 1 page (simplex) or 2 (duplex) | we only have `--pages-multiple-of` |
| cover size | the artwork's bounding box must reach the template's bleed rectangle (0.125 in beyond trim, all four sides, 2 pt tolerance) | needs crop marks in the file to find the template geometry (trim, spine width); without them it **skips** |
| ICC profiles | no ICC outside a PDF/X output intent | printer policy; see G3 in the printer-spec brief |
| spot colours | none allowed | we have `--allow-spot` |
| colour space | RGB is an error | weaker than ours (resource-level only) |
| ink density | warn 240%, error 300% | ours is stricter (per-pixel peak) |
| resolution | error below 200 ppi (72 ppi for interiors) | a different floor per product, which our `--min-dpi` already allows per profile |
| barcode | **a placeholder**: always warns "requires manual verification, ensure the barcode is 100% black vector" | it checks nothing |

Its own TODO comments list what it does not do: bleed uniformity (needs pixel analysis), type safety (text and logos at least
0.25 in inside the trim and fold lines), duplex inside-cover dimensions, and covers with no crop-mark template.

So there is less here to match than the check count suggests. Two of the nine are the interesting ones, and only one of those
is real.

## What proof-pdf already covers for a cover

Fonts, ICC consistency, spots (allowlist), colour space (stricter), ink, resolution (give a cover profile its own
`--min-dpi`), page size and bleed (`--trim-mm`, `--bleed-mm`, `--require-bleed`), artwork reaching the bleed edge on every
side, and text inside a safe margin (`--safe-mm`). A cover is a one-page job with a wide trim; most of the above works today
with a profile in `proof-pdf.toml`.

## What would be new

1. **Wrap geometry.** A flat cover is back + spine + front + bleed. The expected width is `2 x trim width + spine + 2 x bleed`
   and the height is `trim height + 2 x bleed`. The spine width comes from the page count and the stock, and the formula is
   the printer's, so it must be a parameter (`--spine-mm`, or the inputs to a printer-supplied table), never a default. A
   casewrap adds wrap beyond the bleed; its size is also the printer's. Check the declared `TrimBox` and `BleedBox` against
   this, as `check_geometry` does for a card.
2. **Fold and spine zones.** With the geometry known, the page divides into back, spine and front rectangles. Checks that follow:
   text must not straddle a fold; text and logos stay a stated distance inside the trim and from the spine edges (ingram-check
   says 0.25 in, but that is its printer's number); spine text fits the spine width with a margin. This needs the geometry
   from (1) and reuses the paint walk's text extents.
3. **Barcode.** The one genuinely missing capability, and a hard one. Detect a barcode (a cluster of parallel thin black
   vector rectangles, or a raster image of that shape), then verify what a printer cares about: pure K, vector not raster,
   a minimum size, a white quiet zone around it, and its position on the back cover. Decoding the number (zbar or similar)
   would be an optional dependency; start without it. This is where a LaTeX sample library helps: a barcode package for
   LuaLaTeX (to be evaluated; not checked) would generate real ones.
4. **Page count.** `--page-count N` or a min and max, so a cover can be held to 1 or 2 pages. Cheap, and useful beyond covers.
5. **Duplex covers.** Outside and inside pages with their own dimensions. Only if a printer needs it.

## If this is ever picked up

Order: page count (4), then wrap geometry (1) because everything else depends on it, then fold and safe zones (2), then
the barcode (3). Add a `cover` sample group to `tests/fixtures/samples` built from LaTeX (a wrap cover with `tikz` and
`geometry`), with `expected.toml` entries for the passing and failing cases, and run it through the Python API like the rest.
Decide first whether this is a `--cover` mode of `verify` or a `cover` subcommand; ingram-check made it a separate command,
which fits because its option set differs from an interior's.
