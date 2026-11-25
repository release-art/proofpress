# The Rust engine's architecture

Agreed in discussion with the owner, 2026-11-09 to 2026-11-25. This is the shape of the code; the brief
([`../briefs/rust-port.md`](../briefs/rust-port.md)) holds the plan, the stages and the module layout, and was brought in line with
this note on 2026-11-25. The spikes in `spikes/crabify/` are evidence of what is possible, not a design to inherit.

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
`ID`, the caller says how many bytes are binary (`raw(n)`), because only it knows the length.

**Tiers 4 and 5 answer requests** ("object 12"): tier 4 finds the offset and drives tier 2 there. Everything reachable from the
trailer is read at open by a walk that is itself one of the requesters. Tiers 6 to 8 are driven by a page walk.

## The pipeline around each tier

Each tier publishes its events to listeners, called synchronously, in this order:

```text
tier N ──► first-order observers ──► side channel
       └─► proxy (one per tier; absent or identity today)
              └─► second-order readers ──► side channel
                    └─ tier N+1
```

- **First-order observers** see the raw events and report what they find. A tier's validators live here, not in its parser.
- **The proxy** may mute an event. A mute removes what the event covers: a whole subtree if the event is one, or a single event such
  as `DictStart`. The proxy must keep the stream balanced, muting the matching `DictEnd` too; an imbalance seen downstream is a proxy
  bug, reported as an internal finding. All fixing logic for a tier lives in its one proxy. It is a slot for later: the project
  validates and does not repair.
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
  deterministic within a run; they depend on request order, so tests and saved reports match on location, not on id.
- A muted event keeps its id. A synthetic event takes the next number from the same counter.
- Every event carries its location (buffer, offset, length, kind) and its **parent id**: the event it sits inside.
- A table of container events (objects, streams, pages, forms) resolves parent ids. Token-level events are not kept. When a finding
  is raised, its full chain is copied into it, as veraPDF does with its context path.

## Damage

The tier that cannot continue decides; it is a fact about the tier, not an opinion. The proxy cannot override it.

- **File-level damage refuses the file:** no `startxref`, an unreadable xref, encryption, no `/Root`.
- **Object and stream damage leaves a gap,** and reading goes on elsewhere. The file still fails: the structure check reports the
  damage, and every check whose scope holds a gap reports "not evaluated". Nothing is guessed, so this is not repair.
- **Inside a stream, reading stops at the first damage.** There is no resynchronising; the rest of the stream is the gap.
- The tier emits `Damaged { scope, reason, open }` on the main stream, listing every element still open in that scope, and it counts
  as closing them all. Every stream is balanced: each start has its end or is covered by a `Damaged`. The same damage goes to the
  side channel as a finding naming the object or offset.
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

- Each check declares the clauses it may cite. A finding citing one outside its check's set is a bug, caught by a test. The README
  and `--list-checks` list each check's clauses from the declarations.
- A list of clause numbers per standard held, extracted from the test-db Markdown copies, is committed here: numbers only, no titles
  or text. Declared clauses are validated against it.

## Taken from other tools

From a survey on 2026-11-25. Direction agreed; details open.

- **Shape checks from the Arlington PDF Model** (PDF Association): one TSV per object type with keys, types, required keys, values
  and versions, derived from ISO 32000-2. A first-order observer on tier 5, driven by the data. Pin a version; check its licence first.
- **`inspect` as recording listeners** on the same pipeline, as veraPDF does feature extraction, rather than a separate code path.
- **Two verdicts: well-formed** (tiers 1 to 4) **and valid** (tier 5 against Arlington), as JHOVE distinguishes them.
- **More oracles for the parity gate:** the Arlington checker, and perhaps Galois's DaeDaLus PDF parser, next to qpdf.
- Precedent for the proxy: qpdf's `TokenFilter` and MuPDF's filter processor, both muting or rewriting content in a chain. callas
  pdfToolbox and Enfocus PitStop keep checks and fixups separate, as observers and the proxy are here.
- A caution: Caradoc's strict mode refused incremental updates in 65% and object streams in 37% of 10,000 web files. We accept both;
  strictness has to be measured on real files.

## Open

- Does tier 2 take an explicit mode (file body, object stream, content)? `12 0 R` is a reference in a file body and two operands and
  an operator in content.
- Is every tier a public interface, or only a module? Tier 1 has one consumer, so it may be private to tier 2.
- Does `raw(n)` cover every case where tier 2 would otherwise need tier 4 (an indirect `/Length`)?
- Is "not evaluated" a severity or a separate status?
- Do reports show the well-formed and valid verdicts separately?
- Still to discuss: the object model and its sharing with the writer and Python; content streams; the page model; the checks'
  interface, profiles and configuration; limits on depth, size and decompression; PyO3, the CLI, external tools and threads; the
  writer and the other commands.

## Ideas for later

- **A repairing controller** that tells a tier how to carry on after damage, for example "continue this scope". It would sit beside
  the tiers, and is out of scope while the project validates.
- **An edit log of synthetic events** written into any file a proxy changes, as Enfocus's Certified PDF records each session.
