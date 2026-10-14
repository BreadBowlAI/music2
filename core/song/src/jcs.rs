//! JSON in RFC 8785 (JCS) form: the one serialisation a hash of JSON is taken over (WRITE-8's
//! `DeltaHash`, and a Tx's own hash), so the Rust and TypeScript tools agree byte for byte.
//!
//! Object keys sorted by their UTF-16 code units, no white space, strings escaped as
//! ECMAScript's `JSON.stringify` does, and numbers as ECMAScript's `Number#toString` writes them:
//! the shortest digits that read back, `1e+21` from 1e21 up, `1e-7` below 1e-6, never `-0`.

use serde_json::Value;

use crate::Sha;

/// `v` in JCS form. A float that isn't finite can't be JSON, and is never in a `Value`.
pub fn canonical(v: &Value) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

/// The SHA-256 of `v` in JCS form.
pub fn hash(v: &Value) -> Sha {
    Sha::of(canonical(v).as_bytes())
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), ..) => out.push_str(&number(i as f64).unwrap_or_else(|| i.to_string())),
            (_, Some(u), _) => out.push_str(&number(u as f64).unwrap_or_else(|| u.to_string())),
            (.., Some(f)) => out.push_str(&number(f).unwrap_or_else(|| "null".into())),
            _ => out.push_str("null"),
        },
        Value::String(s) => string(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            let mut keys: Vec<(&String, &Value)> = o.iter().collect();
            keys.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            out.push('{');
            for (i, (k, x)) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(k, out);
                out.push(':');
                write(x, out);
            }
            out.push('}');
        }
    }
}

/// A string as `JSON.stringify` writes it: `"` and `\` escaped, the C0 controls as `\b`, `\t`,
/// `\n`, `\f`, `\r` or `\u00xx`, and everything else as it is.
fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `x` as ECMAScript's `Number#toString` writes it (ECMA-262 Number::toString, radix 10), from
/// Rust's shortest round-trip digits. None for a value that isn't finite.
pub fn number(x: f64) -> Option<String> {
    if !x.is_finite() {
        return None;
    }
    if x == 0.0 {
        return Some("0".into());
    }
    let sign = if x < 0.0 { "-" } else { "" };
    // `{:e}` gives the shortest digits that read back: `1.2345e-7`, `5e0`.
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').expect("`{:e}` has an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    // The decimal point sits after `n` digits: x = 0.d1d2…dk × 10^n.
    let n = exp.parse::<i32>().expect("an integer exponent") + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let e = if e >= 0 { format!("+{e}") } else { e.to_string() };
        match k {
            1 => format!("{digits}e{e}"),
            _ => format!("{}.{}e{e}", &digits[..1], &digits[1..]),
        }
    };
    Some(format!("{sign}{body}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // WRITE-8: numbers as ECMAScript writes them, the RFC 8785 appendix's table among them.
    #[test]
    fn numbers_are_written_as_ecmascript_does() {
        let cases: [(f64, &str); 22] = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.3, "0.3"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (4.5, "4.5"),
            (2e-3, "0.002"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (9007199254740992.0, "9007199254740992"),
            (-9.007199254740993e15, "-9007199254740992"),
            (333333333.3333333, "333333333.3333333"),
            (1e23, "1e+23"),
            (2500.0, "2500"),
            (0.045000000000000005, "0.045000000000000005"),
        ];
        for (x, want) in cases {
            assert_eq!(number(x).unwrap(), want, "{x:e}");
        }
        assert_eq!(number(f64::NAN), None);
    }

    // RFC 8785 §3.2.3: keys sorted by UTF-16 code units, no white space, minimal escapes.
    #[test]
    fn objects_and_strings_are_canonical() {
        let v = json!({"\u{20ac}": "Euro Sign", "\r": "Carriage Return", "\u{fb33}": "Hebrew Letter Dalet With Dagesh",
                       "1": "One", "\u{1f600}": "Emoji: Grinning Face", "\u{80}": "Control",
                       "\u{f6}": "Latin Small Letter O With Diaeresis"});
        assert_eq!(
            canonical(&v),
            "{\"\\r\":\"Carriage Return\",\"1\":\"One\",\"\u{80}\":\"Control\",\"\u{f6}\":\"Latin Small Letter O With Diaeresis\",\
             \"\u{20ac}\":\"Euro Sign\",\"\u{1f600}\":\"Emoji: Grinning Face\",\"\u{fb33}\":\"Hebrew Letter Dalet With Dagesh\"}"
        );
        let v = json!({"numbers": [333333333.3333333, 1E30, 4.50, 2e-3, 0.000000000000000000000000001],
                       "string": "\u{20ac}$\u{f}\nA'B\"\\\\\"/", "literals": [null, true, false]});
        assert_eq!(
            canonical(&v),
            "{\"literals\":[null,true,false],\"numbers\":[333333333.3333333,1e+30,4.5,0.002,1e-27],\
             \"string\":\"\u{20ac}$\\u000f\\nA'B\\\"\\\\\\\\\\\"/\"}"
        );
        assert_eq!(canonical(&json!([1, -2, 3.0])), "[1,-2,3]");
    }
}
