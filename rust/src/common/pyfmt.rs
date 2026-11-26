//! Python's number and string formatting where a ported print check's message uses it, so the messages stay the same
//! after the port: `format(x, "g")` and `repr(str)`.

/// `format(x, "g")`: six significant digits, trailing zeros dropped, exponent form below 1e-4 and from 1e6.
pub(crate) fn g(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let scientific = format!("{x:.5e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let trim = |s: &str| {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s.to_owned()
        }
    };
    if !(-4..6).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", trim(mantissa), exponent.abs());
    }
    let decimals = usize::try_from(5 - exponent).unwrap_or(0);
    trim(&format!("{x:.decimals$}"))
}

/// `repr(x)` for a Python float: the shortest digits that read back the same, in fixed notation from 1e-4 to 1e16 (with
/// `.0` when whole), in exponent notation (`1e-05`, `1.5e+16`) beyond.
pub(crate) fn repr_float(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let scientific = format!("{x:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if (-4..16).contains(&exponent) {
        let fixed = format!("{x}");
        return if fixed.contains('.') {
            fixed
        } else {
            format!("{fixed}.0")
        };
    }
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.abs())
}

/// `repr(s)` for a Python string: single quotes unless the string holds a single quote and no double quote, with
/// backslashes, the quote, and control characters escaped (other characters Python counts unprintable are kept).
pub(crate) fn repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let body: String = s
        .chars()
        .map(|c| match c {
            '\\' => "\\\\".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            c if c == quote => format!("\\{c}"),
            // Every control character is below U+0100.
            c if c.is_control() => format!("\\x{:02x}", u32::from(c)),
            c => c.to_string(),
        })
        .collect();
    format!("{quote}{body}{quote}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_python() {
        let cases = [
            (85.0, "85"),
            (85.000_000_000_000_01, "85"),
            (-0.0, "-0"),
            (0.0, "0"),
            (0.1, "0.1"),
            (85.725, "85.725"),
            (297.0011, "297.001"),
            (0.000_01, "1e-05"),
            (0.0001, "0.0001"),
            (1_234_567.0, "1.23457e+06"),
            (999_999.5, "1e+06"),
            (100_000.0, "100000"),
            (2.5e-7, "2.5e-07"),
            (f64::NAN, "nan"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
        ];
        for (x, want) in cases {
            assert_eq!(g(x), want, "{x}");
        }
        let floats = [
            (1.0, "1.0"),
            (0.55, "0.55"),
            (-0.0, "-0.0"),
            (0.0, "0.0"),
            (0.0001, "0.0001"),
            (0.000_015, "1.5e-05"),
            (1e16, "1e+16"),
            (123.25, "123.25"),
            (f64::NAN, "nan"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
        ];
        for (x, want) in floats {
            assert_eq!(repr_float(x), want, "{x}");
        }
        assert_eq!(repr("FOGRA39"), "'FOGRA39'");
        assert_eq!(repr("a'b"), "\"a'b\"");
        assert_eq!(repr("a'b\""), "'a\\'b\"'");
        assert_eq!(repr("t\\a\tb\nc\rd\u{1}e\u{85}"), "'t\\\\a\\tb\\nc\\rd\\x01e\\x85'");
        assert_eq!(repr("é"), "'é'");
    }
}
