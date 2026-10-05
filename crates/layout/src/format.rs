//! Text format mini-language for metric widgets (spec §4.1):
//! literal text, `{value}`, `{value:.N}`, `{value:+.N}`, `{unit}`; `{{`/`}}` escape braces.
use std::fmt::Write as _;

/// Shown instead of a value that is missing (empty state, spec §4.4.1).
pub const EMPTY: &str = "—";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Literal(String),
    /// The metric value with `decimals` digits; `sign` always prints `+`/`-`.
    Value {
        decimals: u8,
        sign: bool,
    },
    /// The symbol of the display unit, e.g. `km/h`.
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("format `{format}`: {message} (at byte {at})")]
pub struct FormatError {
    pub format: String,
    pub at: usize,
    pub message: String,
}

pub fn parse(format: &str) -> Result<Vec<Piece>, FormatError> {
    let err = |at: usize, message: &str| FormatError {
        format: format.to_string(),
        at,
        message: message.to_string(),
    };
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = format.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let next = chars.peek().map(|&(_, n)| n);
        match c {
            '{' if next == Some('{') => {
                chars.next();
                literal.push('{');
            }
            '}' if next == Some('}') => {
                chars.next();
                literal.push('}');
            }
            '}' => return Err(err(i, "unmatched `}` (write `}}` for a literal brace)")),
            '{' => {
                let start = i + 1;
                let end = format[start..]
                    .find('}')
                    .map(|e| start + e)
                    .ok_or_else(|| err(i, "unclosed `{`"))?;
                while chars.peek().is_some_and(|&(j, _)| j <= end) {
                    chars.next();
                }
                if !literal.is_empty() {
                    pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                }
                pieces.push(parse_field(&format[start..end]).map_err(|m| err(i, m))?);
            }
            c => literal.push(c),
        }
    }
    if !literal.is_empty() {
        pieces.push(Piece::Literal(literal));
    }
    Ok(pieces)
}

fn parse_field(field: &str) -> Result<Piece, &'static str> {
    let (name, spec) = match field.split_once(':') {
        Some((n, s)) => (n, Some(s)),
        None => (field, None),
    };
    match (name, spec) {
        ("unit", None) => Ok(Piece::Unit),
        ("unit", Some(_)) => Err("`{unit}` takes no format spec"),
        ("value", None) => Ok(Piece::Value {
            decimals: 0,
            sign: false,
        }),
        ("value", Some(spec)) => {
            let (sign, rest) = match spec.strip_prefix('+') {
                Some(r) => (true, r),
                None => (false, spec),
            };
            if rest.is_empty() {
                return Ok(Piece::Value { decimals: 0, sign });
            }
            let digits = rest
                .strip_prefix('.')
                .ok_or("expected `.N` after `:` (e.g. `{value:.1}`)")?;
            match digits.parse::<u8>() {
                Ok(decimals) if decimals <= 9 => Ok(Piece::Value { decimals, sign }),
                _ => Err("decimals must be a number from 0 to 9"),
            }
        }
        _ => Err("unknown field (expected `value` or `unit`)"),
    }
}

/// Appends the formatted text to `out`. `None` or a non-finite value prints [`EMPTY`].
pub fn apply(out: &mut String, pieces: &[Piece], value: Option<f64>, unit: &str) {
    for piece in pieces {
        match piece {
            Piece::Literal(s) => out.push_str(s),
            Piece::Unit => out.push_str(unit),
            Piece::Value { decimals, sign } => match value.filter(|v| v.is_finite()) {
                None => out.push_str(EMPTY),
                Some(v) => {
                    let d = usize::from(*decimals);
                    let scaled = v * 10f64.powi(i32::from(*decimals));
                    // never print "-0" / "-0.0"
                    let v = if scaled.round() == 0.0 { 0.0 } else { v };
                    let _ = if *sign {
                        write!(out, "{v:+.d$}")
                    } else {
                        write!(out, "{v:.d$}")
                    };
                }
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(format: &str, value: Option<f64>, unit: &str) -> String {
        let mut out = String::new();
        apply(&mut out, &parse(format).unwrap(), value, unit);
        out
    }

    #[test]
    fn parses_value_unit_and_literals() {
        assert_eq!(
            parse("{value:.1} {unit}").unwrap(),
            vec![
                Piece::Value {
                    decimals: 1,
                    sign: false
                },
                Piece::Literal(" ".into()),
                Piece::Unit
            ]
        );
        assert_eq!(
            parse("{value}").unwrap(),
            vec![Piece::Value {
                decimals: 0,
                sign: false
            }]
        );
        assert_eq!(
            parse("{value:+}").unwrap(),
            vec![Piece::Value {
                decimals: 0,
                sign: true
            }]
        );
        assert_eq!(parse("{{x}}").unwrap(), vec![Piece::Literal("{x}".into())]);
        assert_eq!(parse("").unwrap(), vec![]);
    }

    #[test]
    fn rejects_malformed_formats_with_position() {
        for bad in [
            "{value",
            "{speed}",
            "{value:.x}",
            "}",
            "{value:.10}",
            "{unit:.1}",
            "{value:1}",
        ] {
            assert!(parse(bad).is_err(), "{bad} should be rejected");
        }
        let e = parse("ab{speed}").unwrap_err();
        assert_eq!(e.at, 2, "position of the opening brace");
        assert!(e.to_string().contains("ab{speed}"), "{e}");
    }

    #[test]
    fn formats_values() {
        assert_eq!(fmt("{value:.1}", Some(12.345), ""), "12.3");
        assert_eq!(fmt("{value}", Some(47.6), ""), "48");
        assert_eq!(fmt("{value:+.1}%", Some(5.24), ""), "+5.2%");
        assert_eq!(fmt("{value:+.1}", Some(-3.0), ""), "-3.0");
        assert_eq!(fmt("{value:.0} {unit}", Some(48.24), "km/h"), "48 km/h");
        assert_eq!(fmt("{value:.5}°", Some(45.464213), ""), "45.46421°");
    }

    #[test]
    fn values_rounding_to_zero_have_no_minus_sign() {
        assert_eq!(fmt("{value:.1}", Some(-0.04), ""), "0.0");
        assert_eq!(fmt("{value:+.1}", Some(-0.04), ""), "+0.0");
        assert_eq!(fmt("{value:.0}", Some(-0.0), ""), "0");
    }

    #[test]
    fn absent_and_non_finite_values_print_a_dash_and_keep_the_rest() {
        assert_eq!(fmt("{value:.0} {unit}", None, "km/h"), "— km/h");
        assert_eq!(fmt("{value:.1}", Some(f64::NAN), ""), "—");
        assert_eq!(fmt("{value:.1}", Some(f64::INFINITY), ""), "—");
    }

    #[test]
    fn arbitrary_input_never_panics() {
        for s in [
            "{",
            "{{{",
            "}}}",
            "°{value:.1}°{",
            "{:}",
            "{value:+.}",
            "è{è}",
            "{value:.1}}",
            "{value::}",
        ] {
            let _ = parse(s);
        }
    }
}
