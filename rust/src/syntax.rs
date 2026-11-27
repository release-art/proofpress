//! Tiers 1 and 2: tokens and object events (ISO 32000-2, 7.2 and 7.3). Crate-internal: see
//! `docs/notes/rust-architecture.md`.
//!
//! [`Parser`] is a pull parser over one buffer. It never seeks: the caller says where to start and, after `stream` or an
//! inline image's `ID`, how many bytes are binary ([`Parser::raw`]). It has two modes, as qpdf's parser has one flag:
//! in [`Mode::Objects`] `12 0 R` is a reference and a top-level keyword (`obj`, `stream`, `trailer`) is passed up for
//! tier 4 to frame; in [`Mode::Content`] a top-level keyword is an operator and a keyword inside an array or dictionary,
//! a reference among them, is damage. After damage the parser stops: the rest of its scope is a gap.

mod lexer;
mod object;

use crate::findings::{
    Finding, Location, STREAM_EOL, SYNTAX_DUPLICATE_KEY, SYNTAX_HEX_DIGITS, SYNTAX_NAME_ESCAPE, SYNTAX_OCTAL_ESCAPE,
};

pub(crate) use object::{Dict, Note, Object, Read, Value, read};

pub(crate) use lexer::{Defect, Reason as LexReason, is_white};
use lexer::{Kind, Token};

/// Which grammar to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Objects in a file or an object stream.
    Objects,
    /// A content stream.
    Content,
}

/// How a string was written. Both mean the same bytes (7.3.4); the form is kept for findings and the writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// `( ... )`
    Literal,
    /// `< ... >`
    Hex,
}

/// One event. Byte slices borrow the buffer and are still escaped: decode them with [`name_bytes`],
/// [`literal_bytes`] or [`hex_bytes`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Event<'a> {
    /// `null`
    Null,
    /// `true` or `false`
    Bool(bool),
    /// An integer.
    Integer(i64),
    /// A real; its text is the event's span.
    Real(f64),
    /// A name, without its `/`.
    Name(&'a [u8]),
    /// A string, in its form, between its delimiters.
    String(Form, &'a [u8]),
    /// `[`
    ArrayStart,
    /// `]`
    ArrayEnd,
    /// `<<`
    DictStart,
    /// `>>`
    DictEnd,
    /// `12 0 R`, objects mode only.
    Reference(u32, u16),
    /// A top-level keyword in objects mode, such as `obj`, `endobj`, `stream`, `xref` or `trailer`.
    Keyword(&'a [u8]),
    /// A top-level keyword in content mode.
    Operator(&'a [u8]),
}

/// An event and where it is: `start..end` in the buffer, and any byte-level defect it survived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Located<'a> {
    pub(crate) event: Event<'a>,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) defect: Option<Defect>,
}

/// Why the parser stopped. Everything still open at that point is covered by the damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    /// The lexer could not read a token.
    Lexical(LexReason),
    /// A `]` or `>>` that closes nothing, or closes the other kind.
    Unbalanced,
    /// A dictionary key that is not a name, or a key with no value before `>>` (7.3.7).
    BadKey,
    /// A keyword where a value must be: inside an array or dictionary, or `R` without its two integers.
    MisplacedKeyword,
    /// `{` or `}` outside a PostScript calculator function.
    Brace,
    /// Arrays and dictionaries nested deeper than the limit.
    TooDeep,
    /// Binary data longer than what is left of the buffer.
    RawPastEnd,
}

/// Where the parser stopped, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Damage {
    pub(crate) at: usize,
    pub(crate) reason: Reason,
}

/// Binary data read by [`Parser::raw`], and what was irregular around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Raw {
    /// The data's range in the buffer.
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// `stream` was followed by a bare carriage return, which 7.3.8.1 does not allow.
    pub(crate) bare_cr: bool,
    /// The data was not followed by an end of line before what comes next, which 7.3.8.1 requires before `endstream`.
    pub(crate) no_eol_after: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    Array,
    /// A dictionary, and whether the next item is a key.
    Dict {
        key_next: bool,
    },
}

/// A pull parser over one buffer; see the module documentation.
#[derive(Clone)]
pub(crate) struct Parser<'a> {
    buf: &'a [u8],
    pos: usize,
    mode: Mode,
    open: Vec<Open>,
    max_depth: usize,
    /// Tokens read ahead to recognise `n g R`.
    ahead: Vec<Token<'a>>,
    stopped: bool,
}

impl<'a> Parser<'a> {
    /// A parser reading `buf` from `pos` in `mode`, refusing nesting deeper than `max_depth`.
    pub(crate) fn new(buf: &'a [u8], pos: usize, mode: Mode, max_depth: usize) -> Self {
        Self {
            buf,
            pos,
            mode,
            open: Vec::new(),
            max_depth,
            ahead: Vec::new(),
            stopped: false,
        }
    }

    /// Where the next token would be read from.
    pub(crate) fn position(&self) -> usize {
        self.ahead.first().map_or(self.pos, |t| t.start)
    }

    /// The next event, `Ok(None)` at the end of the buffer, or the damage that stops reading. After damage, or the end,
    /// every call returns `Ok(None)`.
    pub(crate) fn next(&mut self) -> Result<Option<Located<'a>>, Damage> {
        if self.stopped {
            return Ok(None);
        }
        let result = self.read();
        if !matches!(result, Ok(Some(_))) {
            self.stopped = true;
        }
        result
    }

    fn token(&mut self) -> Result<Option<Token<'a>>, Damage> {
        if !self.ahead.is_empty() {
            return Ok(Some(self.ahead.remove(0)));
        }
        let token = lexer::next(self.buf, self.pos).map_err(|d| Damage {
            at: d.at,
            reason: Reason::Lexical(d.reason),
        })?;
        if let Some(t) = token {
            self.pos = t.end;
        }
        Ok(token)
    }

    /// The token `n` places ahead, without consuming it. Damage there is not reported yet.
    fn peek(&mut self, n: usize) -> Option<Token<'a>> {
        while self.ahead.len() <= n {
            let token = lexer::next(self.buf, self.pos).ok().flatten()?;
            self.pos = token.end;
            self.ahead.push(token);
        }
        self.ahead.get(n).copied()
    }

    fn read(&mut self) -> Result<Option<Located<'a>>, Damage> {
        let Some(token) = self.token()? else {
            return match self.open.last() {
                None => Ok(None),
                Some(_) => Err(Damage {
                    at: self.buf.len(),
                    reason: Reason::Unbalanced,
                }),
            };
        };
        let damage = |reason| {
            Err(Damage {
                at: token.start,
                reason,
            })
        };
        let event = match token.kind {
            Kind::ArrayClose => return self.close(token, Open::Array, Event::ArrayEnd),
            Kind::DictClose => return self.close(token, Open::Dict { key_next: true }, Event::DictEnd),
            _ if self.open.last() == Some(&Open::Dict { key_next: true }) && !matches!(token.kind, Kind::Name(_)) => {
                return damage(Reason::BadKey);
            }
            Kind::BraceOpen | Kind::BraceClose => return damage(Reason::Brace),
            Kind::ArrayOpen | Kind::DictOpen if self.open.len() >= self.max_depth => return damage(Reason::TooDeep),
            Kind::ArrayOpen => Event::ArrayStart,
            Kind::DictOpen => Event::DictStart,
            Kind::Name(raw) => Event::Name(raw),
            Kind::Literal(raw) => Event::String(Form::Literal, raw),
            Kind::Hex(raw) => Event::String(Form::Hex, raw),
            Kind::Real(r) => Event::Real(r),
            Kind::Integer(i) => match self.reference(i) {
                Some((event, end)) => return Ok(Some(self.emit(event, token.start, end, None))),
                None => Event::Integer(i),
            },
            Kind::Keyword(b"true") => Event::Bool(true),
            Kind::Keyword(b"false") => Event::Bool(false),
            Kind::Keyword(b"null") => Event::Null,
            Kind::Keyword(_) if !self.open.is_empty() => return damage(Reason::MisplacedKeyword),
            Kind::Keyword(b"R") if self.mode == Mode::Objects => return damage(Reason::MisplacedKeyword),
            Kind::Keyword(word) if self.mode == Mode::Objects => Event::Keyword(word),
            Kind::Keyword(word) => Event::Operator(word),
        };
        let located = self.emit(event, token.start, token.end, token.defect);
        match event {
            Event::ArrayStart => self.open.push(Open::Array),
            Event::DictStart => self.open.push(Open::Dict { key_next: true }),
            _ => {}
        }
        Ok(Some(located))
    }

    /// `n g R` starting with the integer `num` just read, in objects mode: the event and where it ends.
    fn reference(&mut self, num: i64) -> Option<(Event<'a>, usize)> {
        if self.mode != Mode::Objects {
            return None;
        }
        let (Some(generation), Some(r)) = (self.peek(0), self.peek(1)) else {
            return None;
        };
        let (Kind::Integer(g), Kind::Keyword(b"R")) = (generation.kind, r.kind) else {
            return None;
        };
        let event = Event::Reference(u32::try_from(num).ok()?, u16::try_from(g).ok()?);
        self.ahead.drain(..2);
        Some((event, r.end))
    }

    /// Record a value or a container's start against the enclosing dictionary's key/value alternation.
    fn emit(&mut self, event: Event<'a>, start: usize, end: usize, defect: Option<Defect>) -> Located<'a> {
        if let Some(Open::Dict { key_next }) = self.open.last_mut() {
            *key_next = !*key_next;
        }
        Located {
            event,
            start,
            end,
            defect,
        }
    }

    fn close(&mut self, token: Token<'a>, kind: Open, event: Event<'a>) -> Result<Option<Located<'a>>, Damage> {
        let matches = match (self.open.last(), kind) {
            // A dictionary may close only after a value, never between a key and its value.
            (Some(Open::Array), Open::Array) | (Some(Open::Dict { key_next: true }), Open::Dict { .. }) => true,
            (Some(Open::Dict { key_next: false }), Open::Dict { .. }) => {
                return Err(Damage {
                    at: token.start,
                    reason: Reason::BadKey,
                });
            }
            _ => false,
        };
        if !matches {
            return Err(Damage {
                at: token.start,
                reason: Reason::Unbalanced,
            });
        }
        self.open.pop();
        Ok(Some(Located {
            event,
            start: token.start,
            end: token.end,
            defect: None,
        }))
    }

    /// Read `len` bytes of binary data after the keyword just read (`stream`, or an inline image's `ID`).
    ///
    /// After `stream` an end of line comes first: CR LF or LF, and a bare CR is noted. After `ID` a single white-space
    /// byte comes first. Then `len` bytes, then an optional end of line, which is noted when missing. Nothing after the
    /// data is consumed: the caller reads `endstream` or `EI` as the next event.
    pub(crate) fn raw(&mut self, len: usize) -> Result<Raw, Damage> {
        let at = self.position();
        self.ahead.clear();
        let rest = self.buf.get(at..).unwrap_or_default();
        let (skip, bare_cr) = match (self.mode, rest) {
            (Mode::Objects, [b'\r', b'\n', ..]) => (2, false),
            (Mode::Objects, [b'\n', ..]) => (1, false),
            (Mode::Objects, [b'\r', ..]) => (1, true),
            (Mode::Content, [b, ..]) if is_white(*b) => (1, false),
            _ => (0, false),
        };
        let start = at + skip;
        let end = start.checked_add(len).filter(|&end| end <= self.buf.len());
        let Some(end) = end else {
            self.stopped = true;
            return Err(Damage {
                at: start,
                reason: Reason::RawPastEnd,
            });
        };
        let after = self.buf.get(end..).unwrap_or_default();
        let eol = match after {
            [b'\r', b'\n', ..] => 2,
            [b'\n' | b'\r', ..] => 1,
            _ => 0,
        };
        self.pos = end + eol;
        Ok(Raw {
            start,
            end,
            bare_cr,
            no_eol_after: eol == 0 && !after.is_empty(),
        })
    }
}

/// Why reading stopped, in words for a finding.
pub(crate) const fn describe(reason: Reason) -> &'static str {
    match reason {
        Reason::Lexical(LexReason::UnterminatedLiteral) => "a literal string is never closed",
        Reason::Lexical(LexReason::UnterminatedHex) => "a hex string has no closing >",
        Reason::Lexical(LexReason::BadHexDigit) => "a hex string holds a character that is not a hex digit",
        Reason::Lexical(LexReason::UnexpectedDelimiter) => "a ) or > where a value should start",
        Reason::Lexical(LexReason::IntegerOverflow) => "an integer too large to read",
        Reason::Unbalanced => "a ] or >> that closes nothing, or closes the other kind",
        Reason::BadKey => "a dictionary key that is not a name, or has no value",
        Reason::MisplacedKeyword => "a keyword where a value should be",
        Reason::Brace => "a { or } outside a PostScript calculator function",
        Reason::TooDeep => "arrays or dictionaries nested deeper than the limit",
        Reason::RawPastEnd => "binary data that runs past the end",
    }
}

/// The finding for a byte-level note, located by `chain` and the note's own offset.
pub(crate) fn note_finding(note: &Note, mut chain: Vec<Location>) -> Finding {
    let (finding, at) = match *note {
        Note::Defect(at, Defect::OddHexDigits) => (
            Finding::new(SYNTAX_HEX_DIGITS, "a hex string has an odd number of digits").citing("7.3.4.3"),
            at,
        ),
        Note::Defect(at, Defect::OctalOverflow) => (
            Finding::new(SYNTAX_OCTAL_ESCAPE, "an octal escape is above 255").citing("7.3.4.2"),
            at,
        ),
        Note::Defect(at, Defect::BadNameEscape) => (
            Finding::new(
                SYNTAX_NAME_ESCAPE,
                "a name has a # not followed by two hex digits, or #00",
            )
            .citing("7.3.5"),
            at,
        ),
        Note::DuplicateKey(at) => (
            Finding::new(SYNTAX_DUPLICATE_KEY, "a dictionary gives a key twice").citing("7.3.7"),
            at,
        ),
        Note::NoEolBeforeEndstream(at) => (
            Finding::new(
                STREAM_EOL,
                "stream data is not followed by an end of line before endstream",
            )
            .citing("7.3.8.1"),
            at,
        ),
    };
    chain.push(Location::Offset(at));
    finding.at(chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn every_reason_has_words() {
        let reasons = [
            Reason::Lexical(LexReason::UnterminatedLiteral),
            Reason::Lexical(LexReason::UnterminatedHex),
            Reason::Lexical(LexReason::BadHexDigit),
            Reason::Lexical(LexReason::UnexpectedDelimiter),
            Reason::Lexical(LexReason::IntegerOverflow),
            Reason::Unbalanced,
            Reason::BadKey,
            Reason::MisplacedKeyword,
            Reason::Brace,
            Reason::TooDeep,
            Reason::RawPastEnd,
        ];
        let words: std::collections::BTreeSet<&str> = reasons.into_iter().map(describe).collect();
        assert_eq!(words.len(), reasons.len());
        assert!(words.contains("a literal string is never closed"));
    }

    fn events(buf: &[u8], mode: Mode) -> (Vec<Event<'_>>, Option<Damage>) {
        let mut parser = Parser::new(buf, 0, mode, 4);
        let mut out = Vec::new();
        loop {
            match parser.next() {
                Ok(Some(located)) => out.push(located.event),
                Ok(None) => return (out, None),
                Err(damage) => {
                    assert_eq!(parser.next(), Ok(None), "the parser stops after damage");
                    return (out, Some(damage));
                }
            }
        }
    }

    #[test]
    fn an_indirect_object() {
        let (got, damage) = events(
            b"12 0 obj << /A 1 0 R /B [true false null 2.5 (s) <61>] >> endobj",
            Mode::Objects,
        );
        assert_eq!(damage, None);
        assert_eq!(
            got,
            [
                Event::Integer(12),
                Event::Integer(0),
                Event::Keyword(b"obj"),
                Event::DictStart,
                Event::Name(b"A"),
                Event::Reference(1, 0),
                Event::Name(b"B"),
                Event::ArrayStart,
                Event::Bool(true),
                Event::Bool(false),
                Event::Null,
                Event::Real(2.5),
                Event::String(Form::Literal, b"s"),
                Event::String(Form::Hex, b"61"),
                Event::ArrayEnd,
                Event::DictEnd,
                Event::Keyword(b"endobj"),
            ]
        );
    }

    #[test]
    fn references_only_in_objects_mode() {
        assert_eq!(
            events(b"[1 2 3]", Mode::Objects).0,
            [
                Event::ArrayStart,
                Event::Integer(1),
                Event::Integer(2),
                Event::Integer(3),
                Event::ArrayEnd
            ]
        );
        assert_eq!(events(b"1 2", Mode::Objects).0, [Event::Integer(1), Event::Integer(2)]);
        assert_eq!(
            events(b"-1 0 R", Mode::Objects).1.map(|d| d.reason),
            Some(Reason::MisplacedKeyword)
        );
        assert_eq!(
            events(b"1 70000 R", Mode::Objects).1.map(|d| d.reason),
            Some(Reason::MisplacedKeyword)
        );
        assert_eq!(
            events(b"1 0 R", Mode::Content).0,
            [Event::Integer(1), Event::Integer(0), Event::Operator(b"R")]
        );
        assert_eq!(
            events(b"[1 0 R]", Mode::Content).1.map(|d| d.reason),
            Some(Reason::MisplacedKeyword)
        );
    }

    #[test]
    fn content_operators() {
        let (got, damage) = events(b"q 1 0 0 1 0 0 cm /F1 12 Tf [(a) -3 (b)] TJ true Q", Mode::Content);
        assert_eq!(damage, None);
        assert_eq!(got.iter().filter(|e| matches!(e, Event::Operator(_))).count(), 5);
        assert!(got.contains(&Event::Bool(true)));
    }

    #[test]
    fn damage() {
        let reason = |buf: &[u8]| events(buf, Mode::Objects).1.map(|d| (d.at, d.reason));
        assert_eq!(reason(b"[1 ]]"), Some((4, Reason::Unbalanced)));
        assert_eq!(reason(b"<< /A >>"), Some((6, Reason::BadKey)));
        assert_eq!(reason(b"<< /A 1 2 >>"), Some((8, Reason::BadKey)));
        assert_eq!(reason(b"[1 >>"), Some((3, Reason::Unbalanced)));
        assert_eq!(
            reason(b"<< /A [1] >"),
            Some((10, Reason::Lexical(LexReason::UnexpectedDelimiter)))
        );
        assert_eq!(reason(b"[1 obj]"), Some((3, Reason::MisplacedKeyword)));
        assert_eq!(reason(b"{"), Some((0, Reason::Brace)));
        assert_eq!(reason(b"[[[[["), Some((4, Reason::TooDeep)));
        assert_eq!(reason(b"<< /A [1"), Some((8, Reason::Unbalanced)));
    }

    #[test]
    fn every_note_is_a_finding() {
        let notes = [
            Note::Defect(1, Defect::OddHexDigits),
            Note::Defect(2, Defect::OctalOverflow),
            Note::Defect(3, Defect::BadNameEscape),
            Note::DuplicateKey(4),
            Note::NoEolBeforeEndstream(5),
        ];
        let found: Vec<_> = notes.iter().map(|n| note_finding(n, vec![Location::Page(1)])).collect();
        let ids: Vec<_> = found.iter().map(|f| f.check.id).collect();
        assert_eq!(
            ids,
            [
                "syntax.hex-digits",
                "syntax.octal-escape",
                "syntax.name-escape",
                "syntax.duplicate-key",
                "stream.eol"
            ]
        );
        assert_eq!(found[4].chain, [Location::Page(1), Location::Offset(5)]);
    }

    #[test]
    fn the_position_counts_tokens_read_ahead() {
        let mut parser = Parser::new(b"1 2 x", 0, Mode::Objects, 4);
        assert_eq!(parser.next().unwrap().unwrap().event, Event::Integer(1));
        assert_eq!(parser.position(), 2);
    }

    #[test]
    fn positions_and_defects() {
        let mut parser = Parser::new(b"  <414> [", 2, Mode::Objects, 4);
        assert_eq!(parser.position(), 2);
        let first = parser.next().unwrap().unwrap();
        assert_eq!(
            (first.start, first.end, first.defect),
            (2, 7, Some(Defect::OddHexDigits))
        );
        assert_eq!(parser.next().unwrap().unwrap().event, Event::ArrayStart);
    }

    fn raw(buf: &[u8], mode: Mode, len: usize) -> (Result<Raw, Damage>, Option<Event<'_>>) {
        let mut parser = Parser::new(buf, 0, mode, 4);
        parser.next().unwrap();
        let raw = parser.raw(len);
        (raw, parser.next().ok().flatten().map(|l| l.event))
    }

    #[test]
    fn binary_data() {
        let ok = |start, end, bare_cr, no_eol_after| {
            Ok(Raw {
                start,
                end,
                bare_cr,
                no_eol_after,
            })
        };
        assert_eq!(
            raw(b"stream\r\nabc\nendstream", Mode::Objects, 3),
            (ok(8, 11, false, false), Some(Event::Keyword(b"endstream")))
        );
        assert_eq!(
            raw(b"stream\nabc\r\nendstream", Mode::Objects, 3).0,
            ok(7, 10, false, false)
        );
        assert_eq!(
            raw(b"stream\rabcendstream", Mode::Objects, 3),
            (ok(7, 10, true, true), Some(Event::Keyword(b"endstream")))
        );
        assert_eq!(raw(b"stream abc", Mode::Objects, 3).0, ok(6, 9, false, true));
        assert_eq!(raw(b"stream\nabc", Mode::Objects, 3).0, ok(7, 10, false, false));
        assert_eq!(
            raw(b"ID \x00\xff EI", Mode::Content, 2),
            (ok(3, 5, false, true), Some(Event::Operator(b"EI")))
        );
        assert_eq!(raw(b"ID", Mode::Content, 0).0, ok(2, 2, false, false));
        let past = raw(b"stream\nabc", Mode::Objects, 9);
        assert_eq!(
            past,
            (
                Err(Damage {
                    at: 7,
                    reason: Reason::RawPastEnd
                }),
                None
            )
        );
    }

    proptest! {
        #[test]
        fn never_panics_and_stays_inside_the_buffer(buf in proptest::collection::vec(any::<u8>(), 0..200), content: bool) {
            let mode = if content { Mode::Content } else { Mode::Objects };
            let mut parser = Parser::new(&buf, 0, mode, 8);
            while let Ok(Some(l)) = parser.next() {
                prop_assert!(l.start < l.end && l.end <= buf.len());
            }
        }
    }
}
