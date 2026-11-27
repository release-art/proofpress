# The Rust engine's architecture

Clause numbers are ISO 32000-2:2020 with Errata Collection 3, held in `proof-pdf-test-db` (`standards/iso/`).

## Tiers

Eight, following the standard. They are modules in one crate, not separate crates.

| # | Tier | Source | Access |
|---|---|---|---|
| 1 | Lexical | 7.2 | Driven by tier 2 |
| 2 | Objects | 7.3 | Pull parser at a position, emitting fine-grained events |
| 3 | Filters | 7.4 | Bytes to bytes |
| 4 | File structure, and the refusal of encryption | 7.5, 7.6 | Request and response |
| 5 | Document structure | 7.7 | Request and response |
| 6 | Content streams | 7.8 | Push |
| 7 | Graphics: the page model and paint events | 8 | Push |
| 8 | Checks | ours | Push |

Helpers sit beside the tiers, not in the stack: common data structures (7.9), functions (7.10), file specifications (7.11), the
extensions dictionary (7.12).

**Tier 2 emits events,** such as `DictStart`, a key, a value, `DictEnd`. A reference (`12 0 R`) is a leaf; following it is tier 5's
job. Tier 2 never seeks: it is given bytes and a position, and the caller decides where to read. After `stream`, or an inline image's
`ID`, the caller says how many bytes are binary (`raw(n)`), because only it knows the length. After `raw`, tier 2 expects an
optional end of line and then `endstream`; a length that runs past the buffer is damage, not a panic.

**Tier 2 has two modes,** as qpdf's parser has one `content_stream` flag:

- **Objects** recognises `n g R` by looking ahead two tokens.
- **Content** makes a bare keyword at the top level an operator. A keyword inside a content array or dictionary is damage, so a
  reference in content (forbidden by 7.8.2) is caught.
- **Object streams need no third mode.** Tier 2 emits `obj`, `endobj`, `stream` and `endstream` as keyword events and tier 4 frames
  them; a `stream` inside an object stream (forbidden by 7.5.7) is tier 4's finding.

**Tiers are modules, not public interfaces.** Tier 1 is private inside tier 2. Tier 2 is crate-internal, with a `#[doc(hidden)]`
entry for fuzzing, as hayro-syntax does. The public surface is tier 5's objects, the listener traits of tiers 6 and 7, and the
checks.

**Tiers 4 and 5 answer requests** ("object 12"): tier 4 finds the offset and drives tier 2 there. Everything reachable from the
trailer is read at open by a walk that is itself one of the requesters. Tiers 6 to 8 are driven by a page walk.

## The pipeline around each tier

Each tier publishes its events to listeners, called synchronously, in this order:

```text
tier N ──► first-order observers ──► side channel
       └─► proxy (one per tier; absent or identity)
              └─► second-order readers ──► side channel
                    └─ tier N+1
```

- **First-order observers** see the raw events and report what they find. A tier's validators live here, not in its parser.
- **The proxy** may mute an event. A mute removes what the event covers: a whole subtree if the event is one, or a single event such
  as `DictStart`. The proxy must keep the stream balanced, muting the matching `DictEnd` too; an imbalance seen downstream is a proxy
  bug, reported as an internal finding. All fixing logic for a tier would live in its one proxy; the project validates and does
  not repair, so no proxy mutes anything.
- **Second-order readers** see what the proxy passed on. The next tier is one of them.
- **Tier 5's cache holds objects as they came out of the proxy,** so a mute holds on the second read.
- **Events are borrowed, not copied,** and listeners are compiled in, chosen per profile.

**The side channel** takes observations from everyone, proxies included, and only the reporter reads it. One exception: a tier's
proxy may read the observations made on the event it is handling, so detection is not written twice. A proxy that mutes says so on
the side channel, after the observer's report of the same event.

## Events

```rust
struct EventId {
    doc: u32,        // index into the document table, which holds the name
    seq: u64,        // global for the document, in parse order
    tier: u8,        // for reading logs; not part of identity
    synthetic: bool, // inserted by a proxy, never found in the file
}
// Identity, equality and hashing are (doc, seq) only.
```

- Every buffer that is opened (the file, or the decoded data of object N) gets an id from a counter per document. Ids are
  deterministic. Objects are all parsed in the same walk at open, so their events get the same ids on every run; content-stream
  events come from page walks and depend on which checks ran, so tests and saved reports match on location, not on id.
- A muted event keeps its id. A synthetic event takes the next number from the same counter.
- Every event carries its location (buffer, offset, length, kind) and its **parent id**: the event it sits inside.
- A table of container events (objects, streams, pages, forms) resolves parent ids. Token-level events are not kept. When a finding
  is raised, its full chain is copied into it, as veraPDF does with its context path.

## The object model

Tier 5 hands out objects built by a second-order reader from tier 2's events, after the proxy. Five designs are taken from other
libraries:

- **Integers and reals stay distinct, and a real keeps its token text** next to its f64, as qpdf does (pikepdf calls its reals
  "exact and lossless"). Checks use the f64; the writer copies the text.
- **Values are slices of their buffer,** as in hayro-syntax, so a nested value's location is its slice's position in the buffer,
  with no event id of its own.
- **Two layers: raw objects and typed views** (`Page`, `Font`, `Image`), as PDFBox has COS and PD. The typed views are derived, as
  pdf-rs derives them, and generated from Arlington's definitions, so the shape checks and the views come from one source.
- **Copying between documents memoises,** as MuPDF's graft map does, so shared children are copied once. Objects are renumbered when
  written, as qpdf does.
- **Dictionaries keep their order,** as lopdf's `IndexMap` does: a key keeps the place where it first appeared.
  A dictionary of 16 or more entries also keeps a map from each key to its place, so a lookup does not scan it; the entries
  change only through `Dict`'s own methods, which keep that map in step.

**Duplicate keys.** ISO 32000-2 7.3.7 forbids them but does not say which copy wins. The last one wins, which is also what qpdf 12
does (it warns "last occurrence overrides earlier ones"). The copies it beat are kept, not lost: every dictionary, nested ones
included, has an `overridden` list of layers, each an ordered dictionary of its own. When a key repeats, its current value moves
down into layer 0, and whatever was there moves down a layer. So

```text
K1 V1   K2 V2   K3 V3   K1 V4   K2 V5   K1 V6
```

gives `{K1: V6, K2: V5, K3: V3}`, with layer 0 `{K1: V4, K2: V2}` and layer 1 `{K1: V1}`. Each duplicate is also a finding.

- **Keys compare after `#xx` escapes are decoded:** `/K#31` and `/K1` are the same key (EC3's note to 7.3.5). qpdf agrees. Each
  occurrence keeps its raw spelling, for findings and the writer.
- **Inline-image keys are normalised** before layering, so `/W` and `/Width` are the same key; the raw spelling is kept.
- **The entries in one dictionary, overridden ones included, count against a limit,** so a key repeated a million times cannot
  exhaust memory.
- **Against qpdf:** qpdf agrees on which value wins and on escaped keys. It does not keep key order, so order is not compared.

**Strings and names.** ISO 32000-2 separates syntax from meaning: literal `(…)` and hex `<…>` mean the same bytes (7.3.4), and
whether those bytes are a text, ASCII, byte or date string depends on the key they sit under (7.9.2), not on how they are written.

- **Every string keeps its form and its raw slice.** The decoded bytes are borrowed from the slice when nothing was escaped and owned
  only when unescaping changed something, as in hayro-syntax. Decoding can lose information (a bare line end in a literal string
  reads as `\n`, 7.3.4.2), so the writer copies the raw slice to reproduce the original exactly.
- **Text is decoded by the typed layer, not the raw one.** A typed view knows from Arlington that `/Title` is `string-text` and
  decodes PDFDocEncoding, UTF-16BE or UTF-8; the raw layer stays bytes.
- **Names are interned per document,** never globally: PDFBox's global name map leaked memory (PDFBOX-5731). The number of distinct
  names counts against a limit.
- **Tier 2 reports byte-level defects** that no later tier can see: a non-hex character in a hex string, an octal escape above
  255, unbalanced parentheses, an invalid `#` escape in a name (`#G1`, a trailing `#`, or `#00`, which 7.3.5 forbids), and strings
  or names over our safety limits.
- **Two defects that only PDF/A forbids are warnings:** an odd number of hex digits (ISO 32000 pads it with a 0; ISO 19005-2
  forbids it) and a name that is not UTF-8 (ISO 19005-2, 6.1.8). PDF/A's verdict stays veraPDF's; the warning says early where it
  will fail.
- **Length limits from standards are checks, not safety limits.** PDF/X-4 (ISO 15930-7, 6.25) requires the PDF 1.6 Reference's
  Table C.1 limits, among them 32,767 bytes per string inside a content stream and 127 per name, so they are PDF/X-4 checks of ours. Under PDF/A alone they stay
  veraPDF's.

**Dangling references: null for meaning, a reason for reporting.** ISO 32000-2 7.3.10 says a reference to an undefined object is not
an error and reads as null, and a null value means the key is absent (7.3.7). Lookups do exactly that. But resolution records why
the result is null: the object is not in the xref or is beyond `/Size`, its entry is free, its generation does not match, the
object's value is null, or the value is a literal `null`. qpdf 12 collapses all five into an absent key with no warning.

- A tier-5 observer reports each dangling reference once, as a **warning**, since the standard allows it but it usually means an
  export or edit went wrong.
- A check that needs the object raises its own error, naming the reason ("`/Im0` points at object 99, which does not exist"), with
  `caused_by` pointing at the warning. `xobject-missing` fires on "nothing usable" whatever the reason; the reason only makes the
  message precise.

**Provenance: one type for every object, with where it came from.** qpdf, lopdf, PDFBox and Roslyn all use one type for parsed and
created objects and leave provenance optional; nobody uses separate types. Ours is an enum, kept per value, since a parsed
dictionary can hold a key a proxy inserted:

| Origin | Records |
|---|---|
| Parsed | Buffer, range and event id |
| Synthetic | The proxy's synthetic event id |
| Created | Who built it: the writer or the API |
| Copied | The source document and the source's own provenance |

- **Bytes are in a buffer or owned:** `InBuffer { buffer, start, len }` or `Owned(...)`, the same split as for unescaped strings.
- **Values with no source text are formatted by the writer:** a created real as the shortest text that reads back as the same f64; a
  created string in the form the writer picks (hex for binary-looking bytes, as qpdf does).
- **Copied keeps its origin,** which pikepdf loses (its `copy_foreign` renumbers at once and keeps no link), so a finding in an
  extracted file can point back to the page it was cut from. Objects are renumbered when written.

**Caches: filled at open, then frozen.** qpdf, PDFBox and MuPDF never share a document between threads; hayro-syntax does, with a
lock-free `OnceLock` per slot. We need neither, because open already reads everything reachable:

- **The walk at open fills the object table,** on one thread and in a fixed order. A stream whose `/Length` points at itself, or two
  objects that need each other, are caught there by an ordinary "currently loading" set. A lazy `OnceLock` cache would deadlock or
  panic on the same input, since re-entering a `OnceLock` from its own initialiser is an error.
- **After open the table is immutable:** a plain `Vec` indexed by object number, `Sync` with no locks.
- **Decoded stream data is cached on first request,** up to a byte budget per document (256 MiB), behind a `Mutex` that is not held
  while decoding.
  Past the budget a stream is decoded afresh each time, so memory stays bounded. Derived data (typed views, page summaries) may
  sit in a `OnceLock` each, as hayro does, since it needs nothing that is still loading.

**Ownership, and the boundary with Python.** A `#[pyclass]` cannot have lifetime parameters, and since PyO3 0.23 it must be `Sync`
for free-threaded Python. So the core holds handles, not borrows, as pikepdf does over qpdf:

- **The document owns a table of buffers:** the file, and the decoded data of each stream. A value stores a buffer id and a range,
  never `&'a [u8]`, and asks the document for its bytes. That is how "a value is a slice of its buffer" is held.
- **No lifetimes in the core,** so the document is `Send` and `Sync`, and `verify` can run with the GIL released (`Python::detach`).
- **Python objects hold an `Arc` of the document** plus an object number, so they keep it alive; there is no "destroyed" state.
- **Python may hand Rust the file's bytes** without a copy: the buffer table holds Python's immutable `bytes` object as the file
  buffer. Anything mutable (`bytearray`, a writable `memoryview`) is copied first, because Python could change it mid-parse.
- **Python can read raw objects,** copied out as plain Python values; Python never sees single operators. Each kind maps as follows,
  close to pikepdf so its users recognise it:

| PDF | Python | Why |
|---|---|---|
| Reference | `Ref(num, gen)`, not followed | Following would copy the whole graph, which can have cycles. Follow with `doc.get(num)` |
| Dictionary | `dict`, in order; the overridden layers as an attribute | Python dicts keep order |
| Stream | Its dictionary and raw bytes; decoded bytes on request | Decoding can be large and can fail |
| Real | `Decimal` from the token text | Exact, as pikepdf does |
| Name | A `Name` type, not `str` | A name and a string differ in PDF |
| String | `bytes`, with its form (literal or hex) as an attribute | Text decoding belongs to the typed views |
| Any value | Its location (buffer, offset) as an attribute | So a Python tool can point at the bytes |


## Damage

The tier that cannot continue decides; it is a fact about the tier, not an opinion. The proxy cannot override it.

- **File-level damage refuses the file:** no `startxref`, an unreadable xref, encryption, no `/Root`.
- **Object and stream damage leaves a gap,** and reading goes on elsewhere. The file still fails: the structure check reports the
  damage, and every check whose scope holds a gap reports "not evaluated". Nothing is guessed, so this is not repair.
- **Inside a stream, reading stops at the first damage.** There is no resynchronising; the rest of the stream is the gap.
- The tier emits `Damaged { scope, reason, open }` on the main stream, listing every element still open in that scope, and it counts
  as closing them all. Every stream is balanced: each start has its end or is covered by a `Damaged`. The same damage goes to the
  side channel as a finding naming the object or offset.
- **A damaged object is partial for reading and a gap for judging.** Tier 5 keeps what was read before the damage (always a prefix,
  since reading stops there) with a marker saying where it stopped. Python, `inspect` and findings show that prefix ("read `/Type`,
  `/Parent`, `/MediaBox`, then damage at offset 214"). To checks and typed views the whole object is a gap: in a partial dictionary
  nothing is certain, since a missing key may lie after the damage and a present one may be overridden by a later duplicate. So
  nothing provisional reaches a verdict. Tree-sitter's `ERROR` nodes are the precedent for marking damage explicitly. qpdf is the
  counter-example: it substitutes nulls or strings and carries on, and with a value missing it shifts keys onto values.
- Each tier turns a `Damaged` it receives into its own for the scope it builds: a broken content stream is a damaged page in tier 7.
  Asking tier 5 for a damaged object returns a damaged result, never a panic.

**Unknowns are findings at every tier:** an unknown key, filter, object type, or operator outside `BX`/`EX`. JHOVE passes what it
does not understand, and gets files wrong because of it.

## Findings

| Field | Note |
|---|---|
| `check` | Stable id, one per rule |
| `severity` | Error or warning |
| `message` | Human text |
| `clause` | Index into a table of standards and editions, plus the clause number: (ISO 32000-2:2020 EC3, 7.3.8.1) |
| `event` | The `EventId` it is about |
| `chain` | Location snapshot from the file down to the event |
| `caused_by` | Optional `EventId` of the cause, so a report can group one defect's consequences |
| `page` | Derived from the chain |

The check catalogue, `Finding`, the per-check outcome and `Report` live in a base module,
`findings`, on top of `pipeline` and below every tier, since every tier raises findings. The JSON is schema 1; its shape is
pinned by an `insta` snapshot (`rust/src/snapshots/`). A finding citing a clause its check does not declare stops a debug
build, so tests catch it.

**Status is separate from severity,** as SARIF separates `kind` from `level`. Every check enabled for a file gets one status:
`pass`, `fail` or `not_evaluated`, with a reason for the last (a gap, a missing tool, a limit, "not measured"). `not_evaluated` fails
the file. The JSON carries a summary per check, so a test can assert that a check was not evaluated; without it, a check that never
ran leaves no trace. "Skipped" is not used: JUnit and TAP both show it as passed.

**One verdict per file:** `pass`, `fail` or `refused`, exiting 0, 1 or 2. Beside it, `structure` is `sound` (tiers 1 to 4 found
nothing), `damaged` (read with gaps) or `refused`. That answers a print shop's question, whether the file is broken or only the
artwork, without a second pass/fail axis. Arlington's shape checks appear as findings.

- Each check declares the clauses it may cite. A finding citing one outside its check's set is a bug, caught by a test. The README
  and `--list-checks` list each check's clauses from the declarations.
- A check reused from X-4 at an older level (PDF/X-1a, X-3) keeps citing its ISO 15930-7 clause in code; its
  `checks.toml` entry maps each such clause to the older part's (`cite.x3-2002 = { "6.8" = "6.4" }`), and `verify`
  rewrites a finding's clause by that map at the end of the run. `build.rs` refuses an entry that applies at an older
  level without mapping every ISO 15930-7 clause it has, so no finding there cites the X-4 text.
- A list of clause numbers per standard held, extracted from the test-db Markdown copies, is committed here: numbers only, no titles
  or text. Declared clauses are validated against it.

## Content streams

- **Inline images.** Keys and the filter and colour-space values are normalised from their abbreviations only inside `BI`; an
  abbreviation used in an image XObject is a finding. A reference, or the JBIG2, JPX or Crypt filter, inside `BI` is damage. A missing
  `/L` in a PDF 2.0 file is a warning (8.9.7 says "shall"); `/L` over 4096 is not a finding, since the limit is a "should".
- **Where an inline image ends.** With `/L`, `raw(L)`, skipping white space first when the last filter is ASCIIHex or ASCII85, then an
  optional white space and `EI`. Without `/L`, the length is measured: width × height × bits per component × components, rounded
  up per row, when unfiltered; otherwise by decoding to the filter's end-of-data marker, so tier 3 reports the bytes it consumed.
  `/L` that disagrees with the measured length is damage. The `EI` guessing every other library uses is not: an ambiguous end is a
  gap.
- **Cycles.** One set of ancestor object numbers covers forms, patterns, Type 3 glyphs, annotation appearances and soft masks. A
  cycle is an error and a gap. Nesting depth is a safety limit; exceeding it reports "not evaluated", never silently.
- **A form without `/Resources` uses the page's resources,** as 7.8.3 says, with a warning. MuPDF does the same; PDFBox and
  pdf.js use the enclosing form's.
- **`BX`/`EX`** keep a nesting counter. An unknown operator outside them is an error; inside, it is ignored and recorded. A pair left
  open at the end of the page is a finding.
- **Several content streams on a page** are read each as its own buffer, with the operand stack and the `q`/`Q`, `BT`/`ET` and `BI`
  state carried across. A token split across two streams is damage (7.7.3.3); no library checks this. An empty `/Contents` array is
  a finding.

## The page model

Tier 7 emits one borrowed, `#[non_exhaustive]` event type, matching tier 2's style: `Paint`, `ClipPush`, `ClipPop`, `GroupBegin`,
`GroupEnd`, `MaskBegin`, `MaskEnd`, `MarkedBegin`, `MarkedEnd` and `Damaged`. Each listener has one `on_event` and ignores what it
does not need. A device trait with one method per kind of paint (hayro, MuPDF, PDFBox) would make fanning out to several listeners
and putting a proxy in between a matter of forwarding every method by hand.

**A paint carries:**

- Provenance: the operator's event id and a frame stack (page, form, pattern, Type 3 glyph, soft mask, annotation).
- Geometry: the CTM and a page-space bbox from true curve extrema, widened by half the line width for strokes.
- The clip's id and bbox.
- Fill and stroke colour separately: resolved space (family, spot names, profiled), components, pattern.
- `OP`, `op` and `OPM` separately.
- Opacity (`ca`, `CA`), blend mode, soft mask, transparency group.
- For strokes, the line width as the minimum page-space width (the smallest singular value of the CTM, not the square root of its
  determinant) and the dash.
- Optional-content membership and visibility. Hidden content is marked, not dropped.
- Marked-content id and artifact.

**Text comes per run,** as in MuPDF and hayro, with a borrowed slice of positioned glyphs (code, Unicode, origin, advance, `.notdef`).
Size and colour are constant within a run, so small-text and contrast checks work per run; per-glyph checks walk the slice.

**Clipping, as hayro models it:** a stack of {path, fill rule, bbox, exact-rectangle flag}. Intersections of rectangles stay exact;
anything else falls back to its bbox, and exact geometry is computed only when a check asks. A form's `/BBox` and text render
modes 4 to 7 are clips too. A finding says when a clip was approximate.

**Transparency is reported, never composited:** group and mask events, and each paint knows whether it is inside a soft mask, whose
content is not ink. The blend-space check runs per group, not per page.

**Text metrics start from widths only** (`/Widths`, `/W`, the standard 14 fonts), with boxes from `FontBBox` or ascent and descent.
A Type 3 glyph's procedure is walked as a nested scope only when a listener asks for it. Font programs are parsed in a
fonts helper, for glyph presence and width consistency.

## Checks, profiles and reports

- **The check list is data:** a checked-in `checks.toml`, read at build time, generates the check table, the README's table and
  `--list-checks` (text, and `--json`). Each check is a `&Check` constant (`X4_TRAPPED_PRESENT`)
  in a list `ALL`, with no generated branches, so the coverage gate has nothing to chase and a misspelt check does not compile. Each entry has an id (kebab-case, stable, never reused), `aliases` for old ids, a title, the
  clauses it may cite, a default severity, and its parameters, each with a type and no default unless the entry gives one with a
  reason and a clause.
- **`proof-pdf.toml`** starts with `schema = 1`; a higher number is refused. A profile may `extends` one other, merged key by key, the
  later value winning, as ESLint merges (changing only a severity keeps the inherited parameters). Checks are named one by one, not
  by prefix.
- **Errors at load name the profile and the key:** an unknown id, a check that is on but missing a parameter with no default, a value
  of the wrong type. `--show-profile` prints the merged result, as ruff's `--show-settings` does. Flags are shorthand for setting a
  check's parameter.
- **Our own versioned JSON stays the main output,** shaped so SARIF can be derived from it without loss: status to `kind`, severity
  to `level`, the chain to logical locations, `caused_by` to related locations. There is no `--format sarif` yet. GitHub's code
  scanning needs line numbers and ignores `kind`, so there the exit code and our JSON stay the real verdict.

## Limits

One `Limits` struct, one field per kind of structure, in one place:

| Group | Kinds |
|---|---|
| Syntax | Direct-object nesting; bytes per string and per name; entries per dictionary, overridden ones included; elements per array; distinct names |
| File | Indirect objects; length of the xref chain |
| Filters | Filters per stream; decoded bytes per stream; decoded-to-encoded ratio; decoded bytes per document |
| Images | Width, height, and width × height × components × bits, checked from the dictionary before decoding |
| Graphs | Page-tree depth and page count; nesting of forms, colour spaces, functions and patterns; depth of outlines and name and number trees |
| Content | Operators per stream; `q` depth; operand stack |
| Time | Per file, and per external tool |

- **Reaching a limit is damage, not a verdict:** `Damaged { reason: Limit(kind) }` for that scope and an error finding
  `limit.<kind>`, so the checks inside report "not evaluated". Only file-level limits (objects, xref chain) refuse the file.
- **Cycles are not limits;** a visited set catches them, whatever the limits are.
- **Decoding stops at its budget** (`Read::take(budget + 1)`). hayro-syntax decodes Flate with no bound.
- **Not configurable:** the defaults are built in (`Limits::default`).
- **Defaults are ours,** chosen to pass every file in the corpus. Not every kind of structure has a limit yet.

## Boundaries

- **Panics.** Each check runs inside `catch_unwind`, so a panic becomes an internal finding and the other checks go on; the frozen
  object table means a panic cannot leave shared state half-written. PyO3 turns an escaped panic into `PanicException`, which
  `except Exception` does not catch. The release profile keeps `panic = "unwind"`.
- **Async.** `core.verify_file` stays `async def` and awaits `asyncio.to_thread`, and Rust releases the GIL inside
  (`Python::detach`). A cancel token, set when the task is cancelled, is polled between pages and while waiting on external tools.
- **Wheels.** Per-version `cp315` wheels, since `requires-python` is 3.15 only; plain abi3 does not load on free-threaded CPython.
- **CLI.** Parse with clap's `try_parse`, so a usage error exits 64 rather than clap's own 2, which would read as "refused". `--help`
  and `--version` exit 0. With several files the worst code wins: refused, then timeout, then findings, then pass. A panic outside
  any check exits 70 (`EX_SOFTWARE`). The wheel's `proof-pdf` command calls the same clap code.
- **External tools** run through `process-wrap`: a session or process group on Unix, a job object on Windows, killed when dropped.
  Output is read on threads up to a cap, with a deadline; what was read is kept for the timeout report. veraPDF gets `-Xmx`.
  `proc.py` goes.
- **Threads.** Files run in parallel (`--jobs`). Within a file, veraPDF and Ghostscript run while the in-process checks do. Pages stay
  sequential, so findings come out in the same order every time.

## The writer

Our own, on the object model:

- **A full rewrite** with a classic xref table and no object streams.
- **Parsed values copy their raw slices,** so a real keeps its text and a string its bytes. Created reals use Rust's shortest `f64`
  text, which never has an exponent (PDF has none). Streams are copied raw with their filters, never re-encoded: that keeps `crop`
  pixel-identical.
- **Copying between documents** goes through a graft map, walked with an explicit stack. A copied page resolves its inherited
  `/Resources`, `/MediaBox` and `/Rotate`, drops `/Parent`, `/Annots` and `/StructParents`, and writes a dangling reference as `null`.
- **Unused resources are pruned** with a tier-6 listener that records the names used; if a content stream is damaged, every resource
  is kept.
- **Every output is read back** by our own parser.
- `pdf-writer` (typst) stores numbers as `f32` only, so it cannot be the copy path; it is fine for building test fixtures.

## Code conventions

For the crate in `rust/`.

- **Public means "crosses the crate".** The crate is not published, and the Python module lives inside it, so it can use
  `pub(crate)` items. Only the CLI binary, the tests in `rust/tests/` and the docs are outside. Default to private or
  `pub(crate)` (the `unreachable_pub` lint enforces this inside private modules). `pub` is for what the binary and the
  behaviour-level tests need: opening a document, running a profile, findings, events, the listener traits. An internal item is
  tested by a unit test next to it, never made `pub` for a test's sake. There is no semver promise; `#[non_exhaustive]` goes
  only on the event enums, so a listener must handle variants added later.
- **Tiers depend only downwards,** enforced by `rust/tests/architecture.rs`, which reads every `crate::` path and fails on an
  edge not in its map. A new module must be added to the map. Two consequences of the map:
  - A tier's own validators live in its module, next to the code they watch (tier 2's byte-level findings in `syntax`).
    `checks` holds the print checks on tiers 5 to 7, and never reaches `syntax`, `filters` or `file`.
  - `document` and `common` may use each other: a page needs rectangles, and rectangles are read from objects.
- **Errors.** A problem in the input is damage and findings, returned as values, never an `Err`. `Result` is only for the
  environment (I/O, configuration, a missing tool), with one typed error enum; never `Result<_, String>`.
- **British spelling in identifiers,** as in the Python code and the user-facing check ids: `colour_space`, not `color_space`. The two halves read alike.
- **No logging in the library.** Printing is denied by lint; everything observable goes through findings and the side channel.
- **Test fixtures.** Rust tests read the existing `tests/fixtures/pdfs/`, not a copy. Event streams and findings get snapshot
  tests (`insta`) once they exist. The corpus in `proof-pdf-test-db` stays out of CI.

PyO3 0.30's generated code compiles under `forbid(unsafe_code)`. One crate with a `python`
feature, `crate-type = ["cdylib", "rlib"]`; maturin builds the extension as `proof_pdf._internal._core` (private, like every
other non-public module), and the version is `rust/Cargo.toml`'s, set from the tag when releasing. The binding is tested in
Rust, which links libpython, so it counts towards the coverage gate like everything else.
