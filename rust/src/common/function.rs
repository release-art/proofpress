//! PDF functions (ISO 32000-2, 7.10), enough to turn a spot colour's tint into the colour it stands in for (ported from
//! the Python `functions` module, under the parity gate): sampled (type 0, nearest sample), exponential (2), stitching
//! (3) and PostScript calculator (4). Anything not understood, or that runs away, gives `None` rather than a wrong
//! colour.

use crate::document::{Dict, Document, Object, Value};

/// A calculator function is a few dozen operators; one that runs longer is malformed.
const MAX_STEPS: usize = 20_000;
/// A sampled function interpolates between at most 2^this many samples.
const MAX_CORNER_INPUTS: usize = 12;
/// How deep stitching functions may nest.
const MAX_DEPTH: usize = 8;

/// The outputs of the function `function` for `inputs`, clipped to its range; `None` when it is of a kind or shape not
/// understood.
pub(crate) fn evaluate(doc: &Document, function: &Object, inputs: &[f64]) -> Option<Vec<f64>> {
    evaluate_at(doc, function, inputs, 0)
}

fn evaluate_at(doc: &Document, function: &Object, inputs: &[f64], depth: usize) -> Option<Vec<f64>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let dict = doc.resolve(function)?.dict()?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a function type is truncated to a whole number"
    )]
    let kind = number(doc, dict.get(b"FunctionType")?)? as i64;
    let domain = numbers(doc, dict.get(b"Domain")?)?;
    // Each input clamped to its interval of the domain, with the interval.
    let x: Vec<(f64, [f64; 2])> = inputs
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let interval = [*domain.get(2 * i)?, *domain.get(2 * i + 1)?];
            Some((v.max(interval[0]).min(interval[1]), interval))
        })
        .collect::<Option<_>>()?;
    let out = match kind {
        2 => exponential(doc, dict, x.first()?.0)?,
        3 => stitching(doc, dict, *x.first()?, depth)?,
        0 => sampled(doc, function, dict, &x)?,
        4 => calculator(doc, function, dict, &x.iter().map(|(v, _)| *v).collect::<Vec<_>>())?,
        _ => return None,
    };
    let Some(range) = dict.get(b"Range") else {
        return Some(out);
    };
    let bounds = numbers(doc, range)?;
    Some(
        out.iter()
            .enumerate()
            .filter_map(|(i, v)| Some(v.max(*bounds.get(2 * i)?).min(*bounds.get(2 * i + 1)?)))
            .collect(),
    )
}

fn number(doc: &Document, object: &Object) -> Option<f64> {
    match doc.resolve(object)?.value {
        #[expect(clippy::cast_precision_loss, reason = "function parameters far below 2^53")]
        Value::Integer(n) => Some(n as f64),
        Value::Real(r) => Some(r),
        _ => None,
    }
}

fn numbers(doc: &Document, object: &Object) -> Option<Vec<f64>> {
    doc.resolve(object)?.array()?.iter().map(|o| number(doc, o)).collect()
}

/// `x` to the power `exponent`, as Python's `**` gives a real result; `None` where it raises or gives a complex one.
fn power(x: f64, exponent: f64) -> Option<f64> {
    if x == 0.0 && exponent < 0.0 || x < 0.0 && exponent.fract() != 0.0 {
        return None;
    }
    // Python raises on overflow rather than giving an infinity.
    Some(x.powf(exponent)).filter(|r| r.is_finite() || !(x.is_finite() && exponent.is_finite()))
}

fn exponential(doc: &Document, dict: &Dict, x: f64) -> Option<Vec<f64>> {
    let c0 = dict.get(b"C0").map_or(Some(vec![0.0]), |c| numbers(doc, c))?;
    let c1 = dict.get(b"C1").map_or(Some(vec![1.0]), |c| numbers(doc, c))?;
    let scale = power(x, number(doc, dict.get(b"N")?)?)?;
    (c0.len() == c1.len()).then(|| c0.iter().zip(&c1).map(|(a, b)| a + scale * (b - a)).collect())
}

fn stitching(doc: &Document, dict: &Dict, (x, [d0, d1]): (f64, [f64; 2]), depth: usize) -> Option<Vec<f64>> {
    let parts = doc.resolve(dict.get(b"Functions")?)?.array()?;
    let bounds = dict.get(b"Bounds").map_or(Some(Vec::new()), |b| numbers(doc, b))?;
    let encode = numbers(doc, dict.get(b"Encode")?)?;
    let mut edges = vec![d0];
    edges.extend(&bounds);
    edges.push(d1);
    let (i, part) = parts
        .iter()
        .enumerate()
        .find(|(k, _)| edges.get(k + 1).is_some_and(|e| x < *e))
        .or_else(|| parts.iter().enumerate().next_back())?;
    let (lo, hi) = (*edges.get(i)?, *edges.get(i + 1)?);
    let (e0, e1) = (*encode.get(2 * i)?, *encode.get(2 * i + 1)?);
    let t = if hi - lo == 0.0 { 0.0 } else { (x - lo) / (hi - lo) };
    evaluate_at(doc, part, &[e0 + t * (e1 - e0)], depth + 1)
}

/// The data of the stream `function` refers to.
fn data(doc: &Document, function: &Object) -> Option<Vec<u8>> {
    match function.value {
        Value::Reference(num, _) => doc.stream_data(num),
        _ => None,
    }
}

/// A whole number from a function's integer parameters; a real is truncated.
fn whole(doc: &Document, object: &Object) -> Option<i64> {
    #[expect(clippy::cast_possible_truncation, reason = "a size is truncated to a whole number")]
    number(doc, object).filter(|n| n.is_finite()).map(|n| n as i64)
}

/// The nearest sample (no interpolation): exact at the tints a spot colour is asked about, 0 and 1.
fn sampled(doc: &Document, function: &Object, dict: &Dict, x: &[(f64, [f64; 2])]) -> Option<Vec<f64>> {
    let size: Vec<i64> = doc
        .resolve(dict.get(b"Size")?)?
        .array()?
        .iter()
        .map(|s| whole(doc, s))
        .collect::<Option<_>>()?;
    let bits = whole(doc, dict.get(b"BitsPerSample")?)?;
    let bounds = numbers(doc, dict.get(b"Range")?)?;
    let outputs = bounds.len() / 2;
    #[expect(clippy::cast_precision_loss, reason = "sample counts far below 2^53")]
    let default_encode: Vec<f64> = size.iter().flat_map(|s| [0.0, (s - 1) as f64]).collect();
    let encode = dict.get(b"Encode").map_or(Some(default_encode), |e| numbers(doc, e))?;
    let decode = dict.get(b"Decode").map_or(Some(bounds.clone()), |d| numbers(doc, d))?;
    let data = data(doc, function)?;
    // Each input's place in the table, clipped to it (7.10.2), as the sample below it, the fraction of the way to the
    // next, and the stride between them. A table too large to index lands past the data and reads as missing.
    let mut places: Vec<(i64, f64, i64)> = Vec::with_capacity(x.len());
    let mut stride = 1i64;
    for (i, (value, [d0, d1])) in x.iter().enumerate() {
        let (e0, e1) = (*encode.get(2 * i)?, *encode.get(2 * i + 1)?);
        let span = d1 - d0;
        let count = *size.get(i)?;
        #[expect(clippy::cast_precision_loss, reason = "sample counts far below 2^53")]
        let e = (e0 + (if span == 0.0 { 0.0 } else { (value - d0) / span }) * (e1 - e0)).clamp(0.0, (count - 1) as f64);
        #[expect(clippy::cast_possible_truncation, reason = "a sample index, inside the table")]
        let below = e.floor() as i64;
        #[expect(clippy::cast_precision_loss, reason = "a sample index, inside the table")]
        let fraction = if below < count - 1 { e - below as f64 } else { 0.0 };
        places.push((below, fraction, stride));
        stride = stride.saturating_mul(count);
    }
    if !(1..=32).contains(&bits) {
        return None;
    }
    let top = (1u64 << bits) - 1;
    let whole_usize = |n: i64| usize::try_from(n).unwrap_or(usize::MAX);
    let sample = |index: i64, j: usize| {
        let start = index
            .saturating_mul(i64::try_from(outputs).unwrap_or(i64::MAX))
            .saturating_add(i64::try_from(j).unwrap_or(i64::MAX))
            .saturating_mul(bits);
        let last = whole_usize(start.saturating_add(bits + 7) / 8).min(data.len());
        let piece = data.get(whole_usize(start / 8)..last).filter(|p| !p.is_empty())?;
        let joined = piece.iter().fold(0u128, |acc, b| acc << 8 | u128::from(*b));
        let width = i64::try_from(piece.len() * 8).unwrap_or(i64::MAX);
        let shift = u32::try_from(width - start % 8 - bits).ok()?;
        #[expect(clippy::cast_possible_truncation, reason = "masked to at most 32 bits")]
        let raw = (joined >> shift) as u64 & top;
        #[expect(clippy::cast_precision_loss, reason = "a sample of at most 32 bits")]
        Some(raw as f64 / top as f64)
    };
    // Multilinear interpolation between the samples around the point (7.10.2; a cubic `/Order` is read as linear):
    // each input that falls between two samples doubles the corners.
    let base = places.iter().fold(0i64, |at, (below, _, stride)| {
        at.saturating_add(below.saturating_mul(*stride))
    });
    let between: Vec<(f64, i64)> = places.iter().filter(|p| p.1 > 0.0).map(|p| (p.1, p.2)).collect();
    if between.len() > MAX_CORNER_INPUTS {
        return None;
    }
    let corners: Vec<(i64, f64)> = (0..1usize << between.len())
        .map(|mask| {
            between
                .iter()
                .enumerate()
                .fold((base, 1.0), |(at, weight), (bit, (fraction, stride))| {
                    if mask >> bit & 1 == 1 {
                        (at.saturating_add(*stride), weight * fraction)
                    } else {
                        (at, weight * (1.0 - fraction))
                    }
                })
        })
        .collect();
    (0..outputs)
        .map(|j| {
            let value = corners
                .iter()
                .map(|(at, weight)| Some(weight * sample(*at, j)?))
                .sum::<Option<f64>>()?;
            let (d0, d1) = (*decode.get(2 * j)?, *decode.get(2 * j + 1)?);
            Some(d0 + value * (d1 - d0))
        })
        .collect()
}

/// A calculator value: a number or a boolean.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Val {
    Num(f64),
    Bool(bool),
}

impl Val {
    fn num(self) -> f64 {
        match self {
            Self::Num(n) => n,
            Self::Bool(b) => f64::from(u8::from(b)),
        }
    }

    fn truthy(self) -> bool {
        match self {
            Self::Num(n) => n != 0.0,
            Self::Bool(b) => b,
        }
    }

    /// Python's `int()`: truncated toward zero; `None` for an infinity or NaN.
    fn int(self) -> Option<i64> {
        let n = self.num();
        #[expect(clippy::cast_possible_truncation, reason = "Python's int() truncates")]
        n.is_finite().then(|| n.trunc() as i64)
    }
}

/// A calculator program: words and the procedures `if` and `ifelse` take.
#[derive(Debug, PartialEq)]
enum Item {
    Word(Vec<u8>),
    Procedure(Vec<Item>),
}

/// The calculator's tokens: braces, and runs of anything else between whitespace (Python's `\s`, vertical tab
/// included).
fn tokens(data: &[u8]) -> Vec<&[u8]> {
    let space = |b: &u8| b.is_ascii_whitespace() || *b == 0x0B;
    let mut out = Vec::new();
    let mut rest = data;
    while let Some(start) = rest.iter().position(|b| !space(b)) {
        rest = rest.get(start..).unwrap_or_default();
        let end = if matches!(rest.first(), Some(b'{' | b'}')) {
            1
        } else {
            rest.iter()
                .position(|b| space(b) || matches!(b, b'{' | b'}'))
                .unwrap_or(rest.len())
        };
        let (token, tail) = rest.split_at(end);
        out.push(token);
        rest = tail;
    }
    out
}

fn parse<'a>(tokens: &mut impl Iterator<Item = &'a [u8]>) -> Vec<Item> {
    let mut items = Vec::new();
    while let Some(token) = tokens.next() {
        match token {
            b"{" => items.push(Item::Procedure(parse(tokens))),
            b"}" => return items,
            word => items.push(Item::Word(word.to_vec())),
        }
    }
    items
}

fn calculator(doc: &Document, function: &Object, dict: &Dict, x: &[f64]) -> Option<Vec<f64>> {
    let outputs = doc.resolve(dict.get(b"Range")?)?.array()?.len() / 2;
    let data = data(doc, function)?;
    let mut program = parse(&mut tokens(&data).into_iter());
    if let [Item::Procedure(only)] = program.as_mut_slice() {
        program = std::mem::take(only);
    }
    let mut stack: Vec<Val> = x.iter().map(|v| Val::Num(*v)).collect();
    run(&program, &mut stack, &mut 0)?;
    let from = if outputs == 0 {
        0
    } else {
        stack.len().saturating_sub(outputs)
    };
    Some(stack.get(from..).unwrap_or_default().iter().map(|v| v.num()).collect())
}

fn run(program: &[Item], stack: &mut Vec<Val>, steps: &mut usize) -> Option<()> {
    let mut i = 0;
    while let Some(item) = program.get(i) {
        *steps += 1;
        if *steps > MAX_STEPS {
            return None;
        }
        i += 1;
        match item {
            Item::Procedure(body) => match (program.get(i), program.get(i + 1)) {
                (Some(Item::Word(w)), _) if w == b"if" => {
                    i += 1;
                    if stack.pop()?.truthy() {
                        run(body, stack, steps)?;
                    }
                }
                (Some(Item::Procedure(other)), Some(Item::Word(w))) if w == b"ifelse" => {
                    i += 2;
                    run(if stack.pop()?.truthy() { body } else { other }, stack, steps)?;
                }
                _ => {}
            },
            Item::Word(word) => operate(word, stack)?,
        }
    }
    Some(())
}

/// Python's `stack[-count:]` start: from the end for a positive count, from the start for a negative one.
fn tail_start(len: usize, count: i64) -> usize {
    bound(len, count.saturating_neg())
}

fn operate(word: &[u8], stack: &mut Vec<Val>) -> Option<()> {
    if let Some(value) = std::str::from_utf8(word).ok().and_then(|w| w.parse::<f64>().ok()) {
        stack.push(Val::Num(value));
        return Some(());
    }
    let unary = |f: fn(f64) -> Option<f64>, stack: &mut Vec<Val>| -> Option<()> {
        let a = stack.pop()?.num();
        stack.push(Val::Num(f(a)?));
        Some(())
    };
    match word {
        b"abs" => unary(|a| Some(a.abs()), stack),
        b"neg" => unary(|a| Some(-a), stack),
        b"ceiling" => unary(|a| Some(a.ceil()), stack),
        b"floor" => unary(|a| Some(a.floor()), stack),
        b"round" => unary(|a| Some((a + 0.5).floor()), stack),
        b"truncate" | b"cvi" => unary(|a| a.is_finite().then(|| a.trunc()), stack),
        b"cvr" => unary(Some, stack),
        b"sqrt" => unary(|a| (a >= 0.0).then(|| a.sqrt()), stack),
        b"sin" => unary(|a| Some(a.to_radians().sin()), stack),
        b"cos" => unary(|a| Some(a.to_radians().cos()), stack),
        b"ln" => unary(|a| (a > 0.0).then(|| a.ln()), stack),
        b"log" => unary(|a| (a > 0.0).then(|| a.log10()), stack),
        b"pop" => stack.pop().map(drop),
        b"dup" => {
            let top = *stack.last()?;
            stack.push(top);
            Some(())
        }
        b"exch" => {
            let (b, a) = (stack.pop()?, stack.pop()?);
            stack.extend([b, a]);
            Some(())
        }
        b"copy" => {
            let count = stack.pop()?.int()?;
            if count != 0 {
                let copied = stack.get(tail_start(stack.len(), count)..).unwrap_or_default().to_vec();
                stack.extend(copied);
            }
            Some(())
        }
        b"index" => {
            let n = stack.pop()?.int()?;
            let len = i64::try_from(stack.len()).unwrap_or(i64::MAX);
            let at = -1 - n;
            let at = if at < 0 { len + at } else { at };
            let value = *stack.get(usize::try_from(at).ok()?)?;
            stack.push(value);
            Some(())
        }
        b"roll" => roll(stack),
        b"not" => {
            let top = stack.pop()?;
            stack.push(match top {
                Val::Bool(b) => Val::Bool(!b),
                #[expect(clippy::cast_precision_loss, reason = "a calculator integer")]
                Val::Num(_) => Val::Num((!top.int()?) as f64),
            });
            Some(())
        }
        b"true" => {
            stack.push(Val::Bool(true));
            Some(())
        }
        b"false" => {
            stack.push(Val::Bool(false));
            Some(())
        }
        _ => {
            let (b, a) = (stack.pop()?, stack.pop()?);
            stack.push(binary(word, a, b)?);
            Some(())
        }
    }
}

/// Python's slice bound `i` on a sequence of `len`: from the end when negative, clamped to the sequence.
fn bound(len: usize, i: i64) -> usize {
    let len_i = i64::try_from(len).unwrap_or(i64::MAX);
    usize::try_from(if i < 0 { (len_i + i).max(0) } else { i.min(len_i) }).unwrap_or(0)
}

/// Python's `a % b` for a divisor that is not zero, whose result takes the divisor's sign.
fn modulo(a: i64, b: i64) -> i64 {
    let r = a.wrapping_rem(b);
    if r != 0 && (r < 0) != (b < 0) { r + b } else { r }
}

/// `count shift roll`, as the Python did it with slices: the top `count` items rotated by `shift`.
fn roll(stack: &mut Vec<Val>) -> Option<()> {
    let shift = stack.pop()?.int()?;
    let count = stack.pop()?.int()?;
    if count == 0 {
        return Some(());
    }
    let mut part = stack.split_off(bound(stack.len(), count.saturating_neg()));
    let shift = modulo(shift, count);
    if shift != 0 {
        let cut = bound(part.len(), shift.saturating_neg());
        part.rotate_left(cut);
    }
    stack.extend(part);
    Some(())
}

#[expect(
    clippy::float_cmp,
    reason = "the calculator's eq and ne compare exactly, as PostScript and the Python did"
)]
fn binary(word: &[u8], lhs: Val, rhs: Val) -> Option<Val> {
    let (x1, x2) = (lhs.num(), rhs.num());
    let num = |v: f64| Some(Val::Num(v));
    let int = |v: Val| v.int();
    match word {
        b"add" => num(x1 + x2),
        b"sub" => num(x1 - x2),
        b"mul" => num(x1 * x2),
        b"div" => (x2 != 0.0).then_some(Val::Num(x1 / x2)),
        b"idiv" => {
            let (i1, i2) = (int(lhs)?, int(rhs)?);
            let floor = i1.checked_div(i2)? - i64::from(modulo(i1, i2) != 0 && (i1 < 0) != (i2 < 0));
            #[expect(clippy::cast_precision_loss, reason = "a quotient of calculator integers")]
            num(floor as f64)
        }
        b"mod" =>
        {
            #[expect(clippy::cast_precision_loss, reason = "a remainder of calculator integers")]
            num(int(lhs)?.checked_rem(int(rhs)?)? as f64)
        }
        b"exp" => num(power(x1, x2)?),
        b"atan" => num(x1.atan2(x2).to_degrees().rem_euclid(360.0)),
        b"eq" => Some(Val::Bool(x1 == x2)),
        b"ne" => Some(Val::Bool(x1 != x2)),
        b"gt" => Some(Val::Bool(x1 > x2)),
        b"ge" => Some(Val::Bool(x1 >= x2)),
        b"lt" => Some(Val::Bool(x1 < x2)),
        b"le" => Some(Val::Bool(x1 <= x2)),
        b"and" | b"or" | b"xor" => {
            if let Val::Bool(flag) = lhs {
                return Some(match word {
                    b"and" => {
                        if flag {
                            rhs
                        } else {
                            lhs
                        }
                    }
                    b"or" => {
                        if flag {
                            lhs
                        } else {
                            rhs
                        }
                    }
                    _ => Val::Bool(x1 != x2),
                });
            }
            let (i1, i2) = (int(lhs)?, int(rhs)?);
            let bits = match word {
                b"and" => i1 & i2,
                b"or" => i1 | i2,
                _ => i1 ^ i2,
            };
            #[expect(clippy::cast_precision_loss, reason = "a calculator integer")]
            num(bits as f64)
        }
        b"bitshift" => {
            let (i1, i2) = (int(lhs)?, int(rhs)?);
            let shifted = if x2 >= 0.0 {
                i1.checked_shl(u32::try_from(i2).ok()?).filter(|s| s >> i2 == i1)?
            } else {
                i1 >> u32::try_from(i2.unsigned_abs()).unwrap_or(u32::MAX).min(63)
            };
            #[expect(clippy::cast_precision_loss, reason = "a calculator integer")]
            num(shifted as f64)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    /// Each function in `functions` (catalog `/F` entries), with streams `more`, evaluated at `inputs`.
    fn eval(function: &str, more: &[(u32, Vec<u8>)], inputs: &[f64]) -> Option<Vec<f64>> {
        let catalog = format!("<< /F {function} >>");
        let mut objects: Vec<(u32, Vec<u8>)> = vec![(1, catalog.into_bytes())];
        objects.extend(more.iter().cloned());
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let doc = Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        evaluate(&doc, doc.catalog().unwrap().get(b"F").unwrap(), inputs)
    }

    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    fn calc(program: &str, inputs: &[f64], outputs: usize) -> Option<Vec<f64>> {
        let range = "0 1000 ".repeat(outputs.max(1)).replace("0 1000", "-1000 1000");
        let range = if outputs == 0 { String::new() } else { range };
        eval(
            "5 0 R",
            &[(
                5,
                stream(
                    &format!("/FunctionType 4 /Domain [-100 100 -100 100] /Range [{range}]"),
                    program.as_bytes(),
                ),
            )],
            inputs,
        )
    }

    #[test]
    fn exponential_and_stitching() {
        let exp = "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0.6 1 0] /N 1 >>";
        assert_eq!(eval(exp, &[], &[1.0]), Some(vec![0.0, 0.6, 1.0, 0.0]));
        assert_eq!(
            eval(exp, &[], &[5.0]),
            Some(vec![0.0, 0.6, 1.0, 0.0]),
            "clamped to the domain"
        );
        assert_eq!(
            eval("<< /FunctionType 2 /Domain [0 1] /N 2 /Range [0 0.5] >>", &[], &[1.0]),
            Some(vec![0.5])
        );
        assert_eq!(
            eval("<< /FunctionType 2 /Domain [0 1] /N -1 >>", &[], &[0.0]),
            None,
            "0 to a negative power"
        );
        assert_eq!(
            eval("<< /FunctionType 2 /Domain [-1 1] /N 0.5 >>", &[], &[-1.0]),
            None,
            "a complex result"
        );
        assert_eq!(
            eval("<< /FunctionType 2 /Domain [0 1] /C0 [0 0] /N 1 >>", &[], &[1.0]),
            None,
            "C0 and C1 differ in length"
        );
        assert_eq!(
            eval("<< /FunctionType 2 /Domain [0 10000000000] /N 400 >>", &[], &[1e10]),
            None,
            "overflow"
        );
        let stitched = "<< /FunctionType 3 /Domain [0 1] /Bounds [0.5] /Encode [0 1 0 1] /Functions [<< /FunctionType 2 /Domain [0 1] /C1 [2] /N 1 >> << /FunctionType 2 /Domain [0 1] /C0 [5] /C1 [7] /N 1 >>] >>";
        assert_eq!(eval(stitched, &[], &[0.25]), Some(vec![1.0]));
        assert_eq!(eval(stitched, &[], &[1.0]), Some(vec![7.0]));
        let flat = "<< /FunctionType 3 /Domain [0 1] /Bounds [1] /Encode [0 1 0 1] /Functions [<< /FunctionType 2 /Domain [0 1] /N 1 >> << /FunctionType 2 /Domain [0 1] /N 1 >>] >>";
        assert_eq!(
            eval(flat, &[], &[1.0]),
            Some(vec![0.0]),
            "an empty last interval maps to its start"
        );
        assert_eq!(
            eval(
                "<< /FunctionType 3 /Domain [0 1] /Encode [] /Functions [] >>",
                &[],
                &[1.0]
            ),
            None
        );
        assert_eq!(eval("<< /FunctionType 7 /Domain [0 1] >>", &[], &[1.0]), None);
        assert_eq!(
            eval("<< /FunctionType 2 /Domain [0] /N 1 >>", &[], &[1.0]),
            None,
            "a short domain"
        );
        let mut deep = "<< /FunctionType 2 /Domain [0 1] /N 1 >>".to_owned();
        for _ in 0..10 {
            deep = format!("<< /FunctionType 3 /Domain [0 1] /Encode [0 1] /Functions [{deep}] >>");
        }
        assert_eq!(eval(&deep, &[], &[1.0]), None, "nested too deep");
        assert_eq!(
            eval("<< /FunctionType 2 /Domain [0 1] /N 1 /Range [/X] >>", &[], &[1.0]),
            None
        );
    }

    #[test]
    fn sampled_functions_interpolate() {
        let one = |at: f64| {
            let f = stream(
                "/FunctionType 0 /Domain [0 1] /Range [0 1] /Size [2] /BitsPerSample 8",
                &[0, 255],
            );
            eval("5 0 R", &[(5, f)], &[at]).map(|v| v[0])
        };
        assert_eq!(one(0.25), Some(0.25));
        assert_eq!(one(1.0), Some(1.0), "the last sample, exactly");
        assert_eq!(one(7.0), Some(1.0), "clipped to the domain");
        let square = stream(
            "/FunctionType 0 /Domain [0 1 0 1] /Range [0 1] /Size [2 2] /BitsPerSample 8",
            &[0, 255, 255, 255],
        );
        let corner = eval("5 0 R", &[(5, square.clone())], &[0.5, 0.5]).map(|v| v[0]);
        assert_eq!(corner, Some(0.75), "the mean of the four samples around the middle");
        assert_eq!(eval("5 0 R", &[(5, square)], &[0.0, 1.0]), Some(vec![1.0]));
        let wide = format!(
            "/FunctionType 0 /Domain [{}] /Range [0 1] /Size [{}] /BitsPerSample 1",
            "0 1 ".repeat(13),
            "2 ".repeat(13)
        );
        let many = eval("5 0 R", &[(5, stream(&wide, &[0; 1024]))], &[0.5; 13]);
        assert_eq!(many, None, "too many inputs between samples");
    }

    #[test]
    fn sampled() {
        let table = |bits: u32, data: &[u8], extra: &str| {
            eval(
                "5 0 R",
                &[(
                    5,
                    stream(
                        &format!(
                            "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1] /Size [2] /BitsPerSample {bits} {extra}"
                        ),
                        data,
                    ),
                )],
                &[1.0],
            )
        };
        assert_eq!(table(8, &[0, 0, 255, 51], ""), Some(vec![1.0, 0.2]));
        assert_eq!(table(4, &[0x00, 0xF5], ""), Some(vec![1.0, 1.0 / 3.0]));
        assert_eq!(
            table(16, &[0; 8], "/Decode [1 0 1 0] /Encode [0 1]"),
            Some(vec![1.0, 1.0])
        );
        assert_eq!(table(8, &[0, 0], ""), None, "a table shorter than its Size");
        assert_eq!(table(0, &[0; 4], ""), None);
        assert_eq!(table(12, &[0, 0, 0, 0xFF], ""), None, "the last sample cut short");
        assert_eq!(
            eval(
                "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [2] /BitsPerSample 8 >>",
                &[],
                &[1.0]
            ),
            None,
            "no stream"
        );
        let zero_span = eval(
            "5 0 R",
            &[(
                5,
                stream(
                    "/FunctionType 0 /Domain [1 1] /Range [0 1] /Size [2] /BitsPerSample 8",
                    &[0, 255],
                ),
            )],
            &[1.0],
        );
        assert_eq!(zero_span, Some(vec![0.0]));
    }

    #[test]
    fn calculator() {
        let one = |p: &str| calc(p, &[], 1).map(|v| v[0]);
        assert_eq!(calc("{ 2 mul exch 1 add }", &[3.0, 4.0], 2), Some(vec![8.0, 4.0]));
        for (program, want) in [
            ("{ -2.5 abs }", 2.5),
            ("{ 2 neg }", -2.0),
            ("{ 1.2 ceiling }", 2.0),
            ("{ 1.8 floor }", 1.0),
            ("{ 2.5 round }", 3.0),
            ("{ -1.7 truncate }", -1.0),
            ("{ -1.7 cvi }", -1.0),
            ("{ 3 cvr }", 3.0),
            ("{ 16 sqrt }", 4.0),
            ("{ 90 sin }", 1.0),
            ("{ 0 cos }", 1.0),
            ("{ 1 ln }", 0.0),
            ("{ 100 log }", 2.0),
            ("{ 5 3 sub }", 2.0),
            ("{ 6 3 div }", 2.0),
            ("{ -7 2 idiv }", -4.0),
            ("{ 7 2 idiv }", 3.0),
            ("{ -7 2 mod }", -1.0),
            ("{ 2 3 exp }", 8.0),
            ("{ 1 1 atan }", 45.0),
            ("{ 0 -1 atan }", 180.0),
            ("{ 5 3 and }", 1.0),
            ("{ 5 3 or }", 7.0),
            ("{ 5 3 xor }", 6.0),
            ("{ 1 3 bitshift }", 8.0),
            ("{ 8 -2 bitshift }", 2.0),
            ("{ 0 not }", -1.0),
            ("{ 1 2 pop }", 1.0),
            ("{ 1 2 dup add add }", 5.0),
            ("{ 1 2 3 2 copy add add add add }", 11.0),
            ("{ 1 2 3 2 index add add add }", 7.0),
            ("{ 1 2 3 3 1 roll pop pop }", 3.0),
            ("{ 1 2 3 3 -1 roll pop pop }", 2.0),
            ("{ 1 2 3 3 0 roll pop pop }", 1.0),
            ("{ 1 2 0 copy add }", 3.0),
            ("{ 1 2 0 0 roll add }", 3.0),
            ("{ 1 2 3 -1 copy pop pop pop add }", 3.0),
            ("{ 1 2 3 -1 1 roll pop pop }", 1.0),
            ("{ 1 2 3 1 -3 index pop add }", 4.0),
            ("{ 1 2 lt { 5 } if }", 5.0),
            ("{ 2 1 lt { 5 } if 7 }", 7.0),
            ("{ 1 2 gt { 5 } { 6 } ifelse }", 6.0),
            ("{ true { 5 } { 6 } ifelse }", 5.0),
            ("{ 1 1 eq { 9 } if }", 9.0),
            ("{ 1 2 ne 1 2 ge or 1 2 le and { 4 } if }", 4.0),
            ("{ true false and false or { 1 } { 2 } ifelse }", 2.0),
            ("{ true 1 xor { 3 } { 4 } ifelse }", 4.0),
            ("{ false 0 or }", 0.0),
            ("{ true 3 and }", 3.0),
            ("{ false not { 1 } if }", 1.0),
            ("{ 1 { 2 } 3 add }", 4.0),
        ] {
            assert_eq!(one(program), Some(want), "{program}");
        }
    }

    #[test]
    fn calculator_failures() {
        for program in [
            "{ -1 sqrt }",
            "{ 0 ln }",
            "{ 0 log }",
            "{ 1 0 div }",
            "{ 1 0 idiv }",
            "{ 1 0 mod }",
            "{ 0 -1 exp }",
            "{ pop }",
            "{ dup }",
            "{ exch }",
            "{ 1 exch }",
            "{ copy }",
            "{ 5 index }",
            "{ 1 roll }",
            "{ roll }",
            "{ not }",
            "{ nonsense }",
            "{ add }",
            "{ 1 add }",
            "{ if }",
            "{ { 1 } if }",
            "{ 1 1 1e300 bitshift }",
            "{ 1e300 1e300 mul cvi }",
            "{ 1e300 1e300 mul 1 and }",
            "{ 1e300 1e300 mul not }",
            "{ 1 9999 bitshift }",
            "{ 1e300 1e300 mul 2 idiv }",
            "{ 1e300 1e300 mul 1 roll }",
            "{ 1e300 1e300 mul copy }",
            "{ 1e300 1e300 mul index }",
        ] {
            assert_eq!(calc(program, &[], 1), None, "{program}");
        }
        assert_eq!(
            calc("{ 1 2 }", &[], 0),
            Some(vec![]),
            "an empty range clips every output"
        );
        assert_eq!(calc("{ 1 }", &[], 3), Some(vec![1.0]), "fewer values than outputs");
        assert_eq!(calc("1 2 add", &[], 1), Some(vec![3.0]), "without braces");
        let looping: String = format!("{{ {} }}", "1 pop ".repeat(15_000));
        assert_eq!(calc(&looping, &[], 1), None, "too many steps");
        assert_eq!(
            eval("<< /FunctionType 4 /Domain [0 1] /Range [0 1] >>", &[], &[0.5]),
            None,
            "no stream"
        );
        assert_eq!(
            eval("<< /FunctionType 4 /Domain [0 1] >>", &[], &[0.5]),
            None,
            "no range"
        );
        assert_eq!(
            tokens(b"{1\x0b2}"),
            [b"{".as_slice(), b"1", b"2", b"}"],
            "a vertical tab separates"
        );
    }

    #[test]
    fn malformed_functions() {
        let f = "<< /FunctionType 2 /Domain [0 1] /N 1 >>";
        let table = |extra: &str, data: &[u8]| {
            eval(
                "5 0 R",
                &[(
                    5,
                    stream(
                        &format!("/FunctionType 0 /Domain [0 1 0 1] /BitsPerSample 8 {extra}"),
                        data,
                    ),
                )],
                &[1.0, 1.0],
            )
        };
        for function in [
            "99 0 R".to_owned(),
            "5".to_owned(),
            "<< /Domain [0 1] >>".to_owned(),
            "<< /FunctionType /X /Domain [0 1] >>".to_owned(),
            "<< /FunctionType 2 >>".to_owned(),
            "<< /FunctionType 2 /Domain 99 0 R /N 1 >>".to_owned(),
            "<< /FunctionType 2 /Domain 5 /N 1 >>".to_owned(),
            "<< /FunctionType 2 /Domain [99 0 R 1] /N 1 >>".to_owned(),
            "<< /FunctionType 2 /Domain [0 1] /C0 5 /N 1 >>".to_owned(),
            "<< /FunctionType 2 /Domain [0 1] /C1 5 /N 1 >>".to_owned(),
            "<< /FunctionType 2 /Domain [0 1] >>".to_owned(),
            "<< /FunctionType 2 /Domain [0 1] /N /X >>".to_owned(),
            "<< /FunctionType 3 /Domain [0 1] /Encode [0 1] >>".to_owned(),
            "<< /FunctionType 3 /Domain [0 1] /Encode [0 1] /Functions 99 0 R >>".to_owned(),
            "<< /FunctionType 3 /Domain [0 1] /Encode [0 1] /Functions 5 >>".to_owned(),
            format!("<< /FunctionType 3 /Domain [0 1] /Encode [0 1] /Bounds 5 /Functions [{f}] >>"),
            format!("<< /FunctionType 3 /Domain [0 1] /Functions [{f}] >>"),
            format!("<< /FunctionType 3 /Domain [0 1] /Encode [0 1 0 1 0 1] /Functions [{f} {f} {f}] >>"),
            format!("<< /FunctionType 3 /Domain [0 1] /Bounds [0.5] /Encode [0 1] /Functions [{f} {f}] >>"),
            format!("<< /FunctionType 3 /Domain [0 1] /Encode 5 /Functions [{f}] >>"),
            format!("<< /FunctionType 3 /Domain [0 1] /Encode [0] /Functions [{f}] >>"),
            format!("<< /FunctionType 3 /Domain [0 1] /Encode [0 1 0 1] /Functions [{f} {f}] >>"),
            "<< /FunctionType 4 /Domain [0 1] /Range 99 0 R >>".to_owned(),
            "<< /FunctionType 4 /Domain [0 1] /Range 5 >>".to_owned(),
        ] {
            assert_eq!(eval(&function, &[], &[1.0]), None, "{function}");
        }
        assert_eq!(eval(f, &[], &[]), None, "no input");
        assert_eq!(
            eval(
                "<< /FunctionType 3 /Domain [0 1] /Encode [0 1] /Functions [] >>",
                &[],
                &[]
            ),
            None
        );
        let two = "<< /FunctionType 2 /Domain [0 1] /C0 [0 0] /C1 [1 1] /N 1 /Range [0 1 0] >>";
        assert_eq!(
            eval(two, &[], &[1.0]),
            Some(vec![1.0]),
            "an output past the range is dropped"
        );
        for (extra, data) in [
            ("/Range [0 1]", &[0u8; 4][..]),
            ("/Size 99 0 R /Range [0 1]", &[0; 4]),
            ("/Size 5 /Range [0 1]", &[0; 4]),
            ("/Size [2 /X] /Range [0 1]", &[0; 4]),
            ("/Size [2 2]", &[0; 4]),
            ("/Size [2 2] /Range [0 1] /Encode 5", &[0; 4]),
            ("/Size [2 2] /Range [0 1] /Decode 5", &[0; 4]),
            ("/Size [2 2] /Range [0 1] /Encode [0 1]", &[0; 4]),
            ("/Size [2] /Range [0 1] /Encode [0 1 0 1]", &[0; 4]),
            ("/Size [2 2] /Range [0 1 0 1] /Decode [0 1]", &[0; 8]),
            ("/Size [2 2] /Range [0 1] /Decode [0]", &[0; 4]),
            ("/Size [2 2] /Range 5", &[0; 4]),
            ("/Size [2 2] /Range [0 1] /Encode [0 1 0]", &[0; 4]),
        ] {
            let got = table(extra, data);
            assert_eq!(got, None, "{extra}");
        }
        let no_bits = eval(
            "5 0 R",
            &[(
                5,
                stream("/FunctionType 0 /Domain [0 1] /Range [0 1] /Size [2]", &[0, 0]),
            )],
            &[1.0],
        );
        assert_eq!(no_bits, None);
        let odd_bits = eval(
            "5 0 R",
            &[(
                5,
                stream(
                    "/FunctionType 0 /Domain [0 1] /Range [0 1] /Size [2] /BitsPerSample /X",
                    &[0, 0],
                ),
            )],
            &[1.0],
        );
        assert_eq!(odd_bits, None);
    }

    #[test]
    fn calculator_edges() {
        let one = |p: &str| calc(p, &[], 1).map(|v| v[0]);
        for (program, want) in [
            ("{ false 3 and }", 0.0),
            ("{ true 3 or }", 1.0),
            ("{ false 3 or }", 3.0),
            ("{ 1 { 5 } if }", 5.0),
        ] {
            assert_eq!(one(program), Some(want), "{program}");
        }
        for program in [
            "{ abs }",
            "{ index }",
            "{ 1 2 -5 index }",
            "{ 2 1e300 1e300 mul mod }",
            "{ 1 2 nonsense }",
            "{ 2 1e300 1e300 mul idiv }",
            "{ 1e300 1e300 mul 2 mod }",
            "{ 1 1e300 1e300 mul and }",
            "{ 1e300 1e300 mul 1 bitshift }",
            "{ 1 1e300 1e300 mul bitshift }",
            "{ 1 { pop pop } if }",
            "{ { 1 } { 2 } ifelse }",
            "{ 1 { pop pop } { 2 } ifelse }",
            "{ 1 1e300 1e300 mul 1 roll }",
            "{ 1 2 1e300 1e300 mul roll }",
        ] {
            assert_eq!(one(program), None, "{program}");
        }
    }
}
