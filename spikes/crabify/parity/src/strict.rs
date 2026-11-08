//! A strict PDF reader for a verifier: no repair, no encryption. Anything the spec does not allow is an error with a reason.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::rc::Rc;

pub type Res<T> = Result<T, String>;
pub type Dict = BTreeMap<Vec<u8>, Obj>;

#[derive(Debug, Clone, PartialEq)]
pub enum Obj {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(Vec<u8>),
    Name(Vec<u8>),
    Array(Vec<Obj>),
    Dict(Dict),
    Stream(Dict, usize, usize), // dictionary, data offset, data length
    Ref(u32, u16),
}

impl Obj {
    pub fn num(&self) -> Option<f64> {
        match self {
            Obj::Int(i) => Some(*i as f64),
            Obj::Real(r) => Some(*r),
            _ => None,
        }
    }
    pub fn dict(&self) -> Option<&Dict> {
        match self {
            Obj::Dict(d) | Obj::Stream(d, ..) => Some(d),
            _ => None,
        }
    }
}

const MAX_NESTING: usize = 100;

fn is_ws(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}
fn is_delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

pub struct Lex<'a> {
    pub d: &'a [u8],
    pub p: usize,
}

fn number(t: &[u8]) -> Option<Obj> {
    let body = t.strip_prefix(b"+").or(t.strip_prefix(b"-")).unwrap_or(t);
    let dots = body.iter().filter(|&&b| b == b'.').count();
    if body.is_empty() || dots > 1 || body == b"." || !body.iter().all(|b| b.is_ascii_digit() || *b == b'.') {
        return None;
    }
    let s = std::str::from_utf8(t).ok()?;
    if dots == 0 {
        // An integer too large for i64 is still a number; qpdf keeps it, so keep its value.
        Some(s.parse::<i64>().map(Obj::Int).unwrap_or(Obj::Real(s.parse().ok()?)))
    } else {
        Some(Obj::Real(s.parse().ok()?))
    }
}

impl<'a> Lex<'a> {
    pub fn new(d: &'a [u8], p: usize) -> Self {
        Self { d, p }
    }
    fn peek(&self) -> Option<u8> {
        self.d.get(self.p).copied()
    }
    pub fn ws(&mut self) {
        while let Some(b) = self.peek() {
            if is_ws(b) {
                self.p += 1;
            } else if b == b'%' {
                while self.peek().is_some_and(|b| b != 10 && b != 13) {
                    self.p += 1;
                }
            } else {
                break;
            }
        }
    }
    pub fn token(&mut self) -> &'a [u8] {
        let s = self.p;
        while self.peek().is_some_and(|b| !is_ws(b) && !is_delim(b)) {
            self.p += 1;
        }
        &self.d[s..self.p]
    }
    pub fn keyword(&mut self, k: &[u8]) -> bool {
        self.ws();
        let s = self.p;
        if self.token() == k {
            return true;
        }
        self.p = s;
        false
    }
    fn name(&mut self) -> Res<Vec<u8>> {
        self.p += 1;
        let t = self.token();
        let mut out = Vec::with_capacity(t.len());
        let mut i = 0;
        while i < t.len() {
            if t[i] == b'#' {
                let h = t.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                let Some(v) = h else { return Err("bad #xx escape in a name".into()) };
                out.push(v);
                i += 3;
            } else {
                out.push(t[i]);
                i += 1;
            }
        }
        Ok(out)
    }
    fn literal(&mut self) -> Res<Vec<u8>> {
        self.p += 1;
        let (mut out, mut depth) = (vec![], 1);
        loop {
            let Some(b) = self.peek() else { return Err("unterminated string".into()) };
            self.p += 1;
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(b);
                }
                b'\r' => {
                    if self.peek() == Some(b'\n') {
                        self.p += 1;
                    }
                    out.push(b'\n');
                }
                b'\\' => {
                    let Some(e) = self.peek() else { return Err("unterminated string".into()) };
                    self.p += 1;
                    match e {
                        b'n' => out.push(10),
                        b'r' => out.push(13),
                        b't' => out.push(9),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\n' => {}
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.p += 1;
                            }
                        }
                        b'0'..=b'7' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(o @ b'0'..=b'7') => {
                                        v = v * 8 + (o - b'0') as u32;
                                        self.p += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        o => out.push(o),
                    }
                }
                o => out.push(o),
            }
        }
    }
    fn hex(&mut self) -> Res<Vec<u8>> {
        self.p += 1;
        let mut digits = vec![];
        loop {
            let Some(b) = self.peek() else { return Err("unterminated hex string".into()) };
            self.p += 1;
            match b {
                b'>' => break,
                b if is_ws(b) => {}
                b if b.is_ascii_hexdigit() => digits.push((b as char).to_digit(16).unwrap() as u8),
                _ => return Err("bad character in a hex string".into()),
            }
        }
        if digits.len() % 2 == 1 {
            digits.push(0);
        }
        Ok(digits.chunks(2).map(|c| c[0] * 16 + c[1]).collect())
    }
    /// One object. *refs* is false in content streams, where `1 0 R` does not exist.
    pub fn value(&mut self, refs: bool, depth: usize) -> Res<Obj> {
        if depth > MAX_NESTING {
            return Err("objects nested too deeply".into());
        }
        self.ws();
        let Some(b) = self.peek() else { return Err("unexpected end of data".into()) };
        match b {
            b'/' => Ok(Obj::Name(self.name()?)),
            b'(' => Ok(Obj::Str(self.literal()?)),
            b'[' => {
                self.p += 1;
                let mut v = vec![];
                loop {
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.p += 1;
                        return Ok(Obj::Array(v));
                    }
                    v.push(self.value(refs, depth + 1)?);
                }
            }
            b'<' if self.d.get(self.p + 1) == Some(&b'<') => {
                self.p += 2;
                let mut d = Dict::new();
                loop {
                    self.ws();
                    if self.d[self.p..].starts_with(b">>") {
                        self.p += 2;
                        return Ok(Obj::Dict(d));
                    }
                    if self.peek() != Some(b'/') {
                        return Err("a dictionary key is not a name".into());
                    }
                    let k = self.name()?;
                    let v = self.value(refs, depth + 1)?;
                    if v != Obj::Null {
                        d.insert(k, v);
                    }
                }
            }
            b'<' => Ok(Obj::Str(self.hex()?)),
            _ => {
                let start = self.p;
                let t = self.token();
                if t.is_empty() {
                    return Err(format!("unexpected byte 0x{b:02x}"));
                }
                match t {
                    b"true" => return Ok(Obj::Bool(true)),
                    b"false" => return Ok(Obj::Bool(false)),
                    b"null" => return Ok(Obj::Null),
                    _ => {}
                }
                let Some(n) = number(t) else {
                    self.p = start;
                    return Err(format!("unexpected token {}", String::from_utf8_lossy(&t[..t.len().min(12)])));
                };
                if let (true, Obj::Int(num)) = (refs, &n) {
                    let save = self.p;
                    self.ws();
                    if let Some(Obj::Int(g)) = number(self.token()) {
                        if self.keyword(b"R") && *num >= 0 && (0..=65535).contains(&g) {
                            return Ok(Obj::Ref(*num as u32, g as u16));
                        }
                    }
                    self.p = save;
                }
                Ok(n)
            }
        }
    }
}

/// One content-stream instruction, or an inline image.
pub struct Op {
    pub name: String,
    pub args: Vec<Obj>,
}

pub fn content(d: &[u8]) -> Res<Vec<Op>> {
    let mut l = Lex::new(d, 0);
    let (mut out, mut args) = (vec![], vec![]);
    loop {
        l.ws();
        let Some(b) = l.peek() else { return Ok(out) };
        if matches!(b, b'/' | b'(' | b'<' | b'[') {
            args.push(l.value(false, 0)?);
            continue;
        }
        let t = l.token();
        if t.is_empty() {
            return Err(format!("unexpected byte 0x{b:02x} in content"));
        }
        match t {
            b"true" => args.push(Obj::Bool(true)),
            b"false" => args.push(Obj::Bool(false)),
            b"null" => args.push(Obj::Null),
            b"BI" => {
                inline_image(&mut l)?;
                args.clear();
                out.push(Op { name: "BI".into(), args: vec![] });
            }
            _ => match number(t) {
                Some(n) => args.push(n),
                None => out.push(Op { name: String::from_utf8_lossy(t).into_owned(), args: std::mem::take(&mut args) }),
            },
        }
    }
}

fn inline_image(l: &mut Lex<'_>) -> Res<()> {
    loop {
        l.ws();
        if l.keyword(b"ID") {
            break;
        }
        if l.peek() != Some(b'/') {
            return Err("an inline image key is not a name".into());
        }
        l.name()?;
        l.value(false, 0)?;
    }
    l.p += 1; // the one white-space byte after ID
    // ponytail: first whitespace-EI-whitespace ends the image; binary data can contain that, /L (PDF 2.0) would settle it
    let mut i = l.p;
    while i + 2 <= l.d.len() {
        let before = i == l.p || is_ws(l.d[i - 1]);
        let after = l.d.get(i + 2).is_none_or(|b| is_ws(*b));
        if &l.d[i..i + 2] == b"EI" && before && after {
            l.p = i + 2;
            return Ok(());
        }
        i += 1;
    }
    Err("inline image has no EI".into())
}

#[derive(Clone, Copy)]
enum Entry {
    Free,
    At(usize, u16),
    InStream(u32),
}

pub struct Doc {
    pub data: Vec<u8>,
    xref: HashMap<u32, Entry>,
    pub trailer: Dict,
    cache: RefCell<HashMap<u32, Rc<Obj>>>,
    loading: RefCell<Vec<u32>>,
}

pub struct PageInfo {
    pub dict: Dict,
    pub media: Option<Obj>,
    pub crop: Option<Obj>,
    pub rotate: Option<Obj>,
    pub resources: Option<Obj>,
}

fn int(o: Option<&Obj>) -> Option<i64> {
    match o {
        Some(Obj::Int(i)) => Some(*i),
        _ => None,
    }
}

impl Doc {
    pub fn open(data: Vec<u8>) -> Res<Self> {
        if !data.starts_with(b"%PDF-") {
            return Err("no %PDF- header at byte 0".into());
        }
        let tail = data.len().saturating_sub(1024);
        let at = data[tail..].windows(9).rposition(|w| w == b"startxref").ok_or("no startxref in the last 1024 bytes")?;
        let mut l = Lex::new(&data, tail + at + 9);
        let Obj::Int(start) = l.value(false, 0).map_err(|_| "startxref has no offset")? else { return Err("startxref has no offset".into()) };
        if !data[tail + at..].windows(5).any(|w| w == b"%%EOF") {
            return Err("no %%EOF".into());
        }
        let mut doc = Doc { xref: HashMap::new(), trailer: Dict::new(), cache: RefCell::new(HashMap::new()), loading: RefCell::new(vec![]), data: vec![] };
        let mut queue = vec![start as usize];
        let mut seen = vec![];
        while let Some(off) = queue.pop() {
            if seen.contains(&off) {
                continue; // a hybrid file's table and stream can share a /Prev, so a repeat is not a loop
            }
            seen.push(off);
            let t = Self::section(&data, off, &mut doc.xref)?;
            if let Some(p) = int(t.get(&b"Prev"[..])) {
                queue.push(p as usize);
            }
            if let Some(p) = int(t.get(&b"XRefStm"[..])) {
                queue.push(p as usize); // popped first: a hybrid file's stream outranks its /Prev
            }
            if doc.trailer.is_empty() {
                doc.trailer = t;
            }
        }
        if doc.trailer.contains_key(&b"Encrypt"[..]) {
            return Err("encrypted".into());
        }
        doc.data = data;
        doc.reachable()?;
        Ok(doc)
    }

    /// Read everything the document can reach, so a broken object is a finding now and not a surprise later.
    /// What nothing references cannot print, so it is not read.
    fn reachable(&self) -> Res<()> {
        let mut todo: Vec<Obj> = self.trailer.values().cloned().collect();
        let mut seen = std::collections::HashSet::new();
        while let Some(o) = todo.pop() {
            match o {
                Obj::Ref(n, _) => {
                    if seen.insert(n) {
                        todo.push((*self.load(n)?).clone());
                    }
                }
                Obj::Array(a) => todo.extend(a),
                Obj::Dict(d) | Obj::Stream(d, ..) => todo.extend(d.into_values()),
                _ => {}
            }
        }
        Ok(())
    }

    fn section(data: &[u8], off: usize, xref: &mut HashMap<u32, Entry>) -> Res<Dict> {
        if off >= data.len() {
            return Err("xref offset past the end of the file".into());
        }
        let mut l = Lex::new(data, off);
        if l.keyword(b"xref") {
            loop {
                if l.keyword(b"trailer") {
                    let Obj::Dict(d) = l.value(true, 0)? else { return Err("trailer is not a dictionary".into()) };
                    return Ok(d);
                }
                let (Obj::Int(first), Obj::Int(count)) = (l.value(false, 0).map_err(|_| "bad xref subsection")?, l.value(false, 0).map_err(|_| "bad xref subsection")?) else {
                    return Err("bad xref subsection".into());
                };
                for i in 0..count {
                    let (Obj::Int(o), Obj::Int(g)) = (l.value(false, 0).map_err(|_| "bad xref entry")?, l.value(false, 0).map_err(|_| "bad xref entry")?) else {
                        return Err("bad xref entry".into());
                    };
                    l.ws();
                    let e = match l.token() {
                        b"n" => Entry::At(o as usize, g as u16),
                        b"f" => Entry::Free,
                        _ => return Err("bad xref entry".into()),
                    };
                    xref.entry((first + i) as u32).or_insert(e);
                }
            }
        }
        let (d, s, n) = Self::indirect(data, off, None, None).map_err(|_| "startxref does not point at an xref section")?;
        let Obj::Stream(dict, ..) = &d else { return Err("startxref does not point at an xref section".into()) };
        if dict.get(&b"Type"[..]) != Some(&Obj::Name(b"XRef".to_vec())) {
            return Err("startxref does not point at an xref section".into());
        }
        let raw = decode(&data[s..s + n], dict, &|o| Ok(Rc::new(o.clone())))?;
        let Some(Obj::Array(w)) = dict.get(&b"W"[..]) else { return Err("xref stream has no /W".into()) };
        let w: Vec<usize> = w.iter().filter_map(|x| x.num().map(|v| v as usize)).collect();
        let [w0, w1, w2] = w[..] else { return Err("xref stream /W is not three numbers".into()) };
        let size = int(dict.get(&b"Size"[..])).ok_or("xref stream has no /Size")?;
        let index: Vec<i64> = match dict.get(&b"Index"[..]) {
            Some(Obj::Array(a)) => a.iter().filter_map(|x| x.num().map(|v| v as i64)).collect(),
            _ => vec![0, size],
        };
        let width = w0 + w1 + w2;
        let be = |b: &[u8]| b.iter().fold(0u64, |a, v| a << 8 | *v as u64);
        let mut rows = raw.chunks_exact(width.max(1));
        for pair in index.chunks(2) {
            let [first, count] = pair else { return Err("xref stream /Index is odd".into()) };
            for i in 0..*count {
                let r = rows.next().ok_or("xref stream is shorter than /Index says")?;
                let kind = if w0 == 0 { 1 } else { be(&r[..w0]) };
                let (a, b) = (be(&r[w0..w0 + w1]), be(&r[w0 + w1..]));
                let e = match kind {
                    0 => Entry::Free,
                    1 => Entry::At(a as usize, b as u16),
                    2 => Entry::InStream(a as u32),
                    _ => continue, // reserved types are null objects
                };
                xref.entry((first + i) as u32).or_insert(e);
            }
        }
        Ok(dict.clone())
    }

    /// The object at *off*; a stream needs *resolve* only when its /Length is indirect.
    fn indirect(data: &[u8], off: usize, want: Option<u32>, doc: Option<&Doc>) -> Res<(Obj, usize, usize)> {
        if off == 0 || off >= data.len() {
            return Err("xref offset is outside the file".into());
        }
        let mut l = Lex::new(data, off);
        let (n, g) = (l.value(false, 0), l.value(false, 0));
        let (Ok(Obj::Int(n)), Ok(Obj::Int(_))) = (n, g) else { return Err("xref offset does not point at an object".into()) };
        if !l.keyword(b"obj") || want.is_some_and(|w| w != n as u32) {
            return Err("xref offset does not point at the right object".into());
        }
        let o = l.value(true, 0)?;
        if l.keyword(b"endobj") {
            return Ok((o, 0, 0));
        }
        if !l.keyword(b"stream") {
            return Err("object has no endobj".into());
        }
        let Obj::Dict(d) = o else { return Err("stream without a dictionary".into()) };
        let s = if data[l.p..].starts_with(b"\r\n") {
            l.p + 2
        } else if data.get(l.p) == Some(&b'\n') {
            l.p + 1
        } else {
            return Err("stream keyword not followed by an end of line".into());
        };
        let len = match (d.get(&b"Length"[..]), doc) {
            (Some(Obj::Int(n)), _) => *n as usize,
            (Some(Obj::Ref(r, _)), Some(doc)) => match &*doc.load(*r)? {
                Obj::Int(n) => *n as usize,
                _ => return Err("stream /Length is not a number".into()),
            },
            _ => return Err("stream has no usable /Length".into()),
        };
        let end = s.checked_add(len).filter(|e| *e <= data.len()).ok_or("stream /Length runs past the end of the file")?;
        if !Lex::new(data, end).keyword(b"endstream") {
            return Err("stream /Length is wrong".into());
        }
        Ok((Obj::Stream(d, s, len), s, len))
    }

    pub fn load(&self, n: u32) -> Res<Rc<Obj>> {
        if let Some(o) = self.cache.borrow().get(&n) {
            return Ok(o.clone());
        }
        // An object that needs itself to load (a /Length pointing at its own stream) is an error, not a recursion.
        if self.loading.borrow().contains(&n) {
            return Err("an object depends on itself".into());
        }
        self.loading.borrow_mut().push(n);
        let found = self.read(n);
        self.loading.borrow_mut().pop();
        found
    }

    fn read(&self, n: u32) -> Res<Rc<Obj>> {
        let o = match self.xref.get(&n) {
            None | Some(Entry::Free) => Obj::Null,
            Some(Entry::At(off, _)) => Self::indirect(&self.data, *off, Some(n), Some(self))?.0,
            Some(Entry::InStream(stm)) => {
                self.unpack(*stm)?;
                return self.cache.borrow().get(&n).cloned().ok_or("object is not in its object stream".to_string());
            }
        };
        let o = Rc::new(o);
        self.cache.borrow_mut().insert(n, o.clone());
        Ok(o)
    }

    fn unpack(&self, stm: u32) -> Res<()> {
        let s = self.load(stm)?;
        let Obj::Stream(d, ..) = &*s else { return Err("object stream is not a stream".into()) };
        let body = self.stream(&s)?;
        let (Some(count), Some(first)) = (self.int(d, b"N")?, self.int(d, b"First")?) else { return Err("object stream lacks /N or /First".into()) };
        let mut head = Lex::new(&body, 0);
        for _ in 0..count {
            let (Obj::Int(num), Obj::Int(off)) = (head.value(false, 0)?, head.value(false, 0)?) else { return Err("bad object stream header".into()) };
            let o = Lex::new(&body, (first + off) as usize).value(true, 0)?;
            // The xref decides which copy is current; an older one in this stream must not shadow it.
            if matches!(self.xref.get(&(num as u32)), Some(Entry::InStream(s)) if *s == stm) {
                self.cache.borrow_mut().insert(num as u32, Rc::new(o));
            }
        }
        Ok(())
    }

    fn int(&self, d: &Dict, k: &[u8]) -> Res<Option<i64>> {
        Ok(self.get(d, k)?.and_then(|o| int(Some(&o))))
    }

    pub fn resolve(&self, o: &Obj) -> Res<Rc<Obj>> {
        match o {
            Obj::Ref(n, _) => self.load(*n),
            o => Ok(Rc::new(o.clone())),
        }
    }

    pub fn get(&self, d: &Dict, k: &[u8]) -> Res<Option<Obj>> {
        match d.get(k) {
            None => Ok(None),
            Some(o) => Ok(match &*self.resolve(o)? {
                Obj::Null => None,
                o => Some(o.clone()),
            }),
        }
    }

    pub fn raw<'s>(&'s self, o: &Obj) -> &'s [u8] {
        match o {
            Obj::Stream(_, s, n) => &self.data[*s..*s + *n],
            _ => &[],
        }
    }

    pub fn stream(&self, o: &Obj) -> Res<Vec<u8>> {
        let Obj::Stream(d, ..) = o else { return Err("not a stream".into()) };
        decode(self.raw(o), d, &|x| self.resolve(x))
    }

    pub fn pages(&self) -> Res<Vec<PageInfo>> {
        let root = self.get(&self.trailer, b"Root")?.ok_or("no /Root")?;
        let root = root.dict().ok_or("/Root is not a dictionary")?.clone();
        let tree = self.get(&root, b"Pages")?.ok_or("no /Pages")?;
        let mut out = vec![];
        let mut seen = vec![];
        self.node(&root, &tree, [None, None, None, None], &mut seen, &mut out, root.get(&b"Pages"[..]).cloned())?;
        Ok(out)
    }

    fn node(&self, root: &Dict, n: &Obj, inh: [Option<Obj>; 4], seen: &mut Vec<u32>, out: &mut Vec<PageInfo>, me: Option<Obj>) -> Res<()> {
        if let Some(Obj::Ref(r, _)) = me {
            if seen.contains(&r) {
                return Err("the page tree loops".into());
            }
            seen.push(r);
        }
        let d = n.dict().ok_or("a page tree node is not a dictionary")?;
        let mut inh = inh;
        for (slot, key) in inh.iter_mut().zip([&b"MediaBox"[..], b"CropBox", b"Rotate", b"Resources"]) {
            if let Some(v) = self.get(d, key)? {
                *slot = Some(v);
            }
        }
        match self.get(d, b"Kids")? {
            Some(Obj::Array(kids)) => {
                for k in &kids {
                    let child = self.resolve(k)?;
                    self.node(root, &child, inh.clone(), seen, out, Some(k.clone()))?;
                }
            }
            _ => {
                let [media, crop, rotate, resources] = inh;
                out.push(PageInfo { dict: d.clone(), media, crop, rotate, resources });
            }
        }
        Ok(())
    }
}

fn params(d: &Dict, i: usize, resolve: &dyn Fn(&Obj) -> Res<Rc<Obj>>) -> Res<Dict> {
    let Some(p) = d.get(&b"DecodeParms"[..]).or(d.get(&b"DP"[..])) else { return Ok(Dict::new()) };
    let p = resolve(p)?;
    let one = match &*p {
        Obj::Array(a) => match a.get(i) {
            Some(o) => resolve(o)?,
            None => return Ok(Dict::new()),
        },
        _ => p.clone(),
    };
    Ok(one.dict().cloned().unwrap_or_default())
}

pub fn filter_names(d: &Dict, resolve: &dyn Fn(&Obj) -> Res<Rc<Obj>>) -> Res<Vec<Vec<u8>>> {
    let Some(f) = d.get(&b"Filter"[..]).or(d.get(&b"F"[..]).filter(|f| matches!(f, Obj::Name(_) | Obj::Array(_)))) else { return Ok(vec![]) };
    Ok(match &*resolve(f)? {
        Obj::Name(n) => vec![n.clone()],
        Obj::Array(a) => {
            let mut v = vec![];
            for x in a {
                if let Obj::Name(n) = &*resolve(x)? {
                    v.push(n.clone());
                }
            }
            v
        }
        _ => vec![],
    })
}

fn decode(data: &[u8], d: &Dict, resolve: &dyn Fn(&Obj) -> Res<Rc<Obj>>) -> Res<Vec<u8>> {
    let mut cur = data.to_vec();
    for (i, f) in filter_names(d, resolve)?.iter().enumerate() {
        let p = params(d, i, resolve)?;
        cur = match f.as_slice() {
            b"FlateDecode" | b"Fl" => {
                let mut out = vec![];
                flate2::read::ZlibDecoder::new(&cur[..]).read_to_end(&mut out).map_err(|e| format!("Flate data is damaged: {e}"))?;
                predict(out, &p, resolve)?
            }
            b"LZWDecode" | b"LZW" => {
                let early = p.get(&b"EarlyChange"[..]).and_then(|o| o.num()).unwrap_or(1.0) != 0.0;
                let mut dec = if early {
                    weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
                } else {
                    weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8)
                };
                predict(dec.decode(&cur).map_err(|e| format!("LZW data is damaged: {e}"))?, &p, resolve)?
            }
            b"ASCII85Decode" | b"A85" => ascii85(&cur)?,
            b"ASCIIHexDecode" | b"AHx" => {
                let body: Vec<u8> = cur.iter().copied().take_while(|b| *b != b'>').filter(|b| !is_ws(*b)).collect();
                let mut l = Lex::new(&body, 0);
                let mut v = vec![b'<'];
                v.extend_from_slice(l.d);
                v.push(b'>');
                Lex::new(&v, 0).hex()?
            }
            b"RunLengthDecode" | b"RL" => {
                let (mut out, mut i) = (vec![], 0);
                while i < cur.len() && cur[i] != 128 {
                    let n = cur[i] as usize;
                    if n < 128 {
                        out.extend_from_slice(cur.get(i + 1..i + 2 + n).ok_or("RunLength data is short")?);
                        i += n + 2;
                    } else {
                        out.extend(std::iter::repeat_n(*cur.get(i + 1).ok_or("RunLength data is short")?, 257 - n));
                        i += 2;
                    }
                }
                out
            }
            b"Crypt" => cur,
            other => return Err(format!("unsupported filter {}", String::from_utf8_lossy(other))),
        };
    }
    Ok(cur)
}

fn ascii85(d: &[u8]) -> Res<Vec<u8>> {
    let (mut out, mut group) = (vec![], vec![]);
    for &b in d {
        match b {
            b'~' => break,
            b'z' if group.is_empty() => out.extend_from_slice(&[0; 4]),
            b'!'..=b'u' => {
                group.push((b - b'!') as u32);
                if group.len() == 5 {
                    let v = group.iter().fold(0u32, |a, x| a.wrapping_mul(85).wrapping_add(*x));
                    out.extend_from_slice(&v.to_be_bytes());
                    group.clear();
                }
            }
            b if is_ws(b) => {}
            _ => return Err("bad character in ASCII85 data".into()),
        }
    }
    if group.len() > 1 {
        let n = group.len();
        group.resize(5, 84);
        let v = group.iter().fold(0u32, |a, x| a.wrapping_mul(85).wrapping_add(*x));
        out.extend_from_slice(&v.to_be_bytes()[..n - 1]);
    }
    Ok(out)
}

fn predict(data: Vec<u8>, p: &Dict, resolve: &dyn Fn(&Obj) -> Res<Rc<Obj>>) -> Res<Vec<u8>> {
    let num = |k: &[u8], default: usize| -> Res<usize> {
        Ok(match p.get(k) {
            Some(o) => resolve(o)?.num().map(|v| v as usize).unwrap_or(default),
            None => default,
        })
    };
    let predictor = num(b"Predictor", 1)?;
    if predictor < 2 {
        return Ok(data);
    }
    let (colors, bpc, columns) = (num(b"Colors", 1)?, num(b"BitsPerComponent", 8)?, num(b"Columns", 1)?);
    let row = (colors * bpc * columns).div_ceil(8);
    let bpp = (colors * bpc).div_ceil(8);
    if predictor == 2 {
        if bpc != 8 {
            return Err("TIFF predictor below 8 bits is not implemented in this spike".into());
        }
        let mut d = data;
        for r in d.chunks_mut(row) {
            for i in bpp..r.len() {
                r[i] = r[i].wrapping_add(r[i - bpp]);
            }
        }
        return Ok(d);
    }
    let mut out: Vec<u8> = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; row];
    for r in data.chunks(row + 1) {
        let (tag, src) = (r[0], &r[1..]);
        let mut cur = vec![0u8; row];
        cur[..src.len()].copy_from_slice(src);
        for i in 0..src.len() {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let add = match tag {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => {
                    let (pa, pb, pc) = ((b as i32 - c as i32).abs(), (a as i32 - c as i32).abs(), (a as i32 + b as i32 - 2 * c as i32).abs());
                    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                }
                _ => return Err("bad PNG predictor tag".into()),
            };
            cur[i] = cur[i].wrapping_add(add);
        }
        out.extend_from_slice(&cur[..src.len()]);
        prev = cur;
    }
    Ok(out)
}
