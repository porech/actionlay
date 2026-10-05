//! Text format mini-language for metric widgets (spec §4.1):
//! literal text, `{value}`, `{value:.N}`, `{value:+.N}`, `{unit}`; `{{`/`}}` escape braces.
use std::fmt::Write as _;

/// Shown instead of a value that is missing (empty state, spec §4.4.1).
pub const EMPTY: &str = "—";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Literal(String),
    /// Minutes per distance displayed as m:ss.
    Pace,
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
        ("value", Some("pace")) => Ok(Piece::Pace),
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
            match digits.as_bytes() {
                [d @ b'0'..=b'9'] => Ok(Piece::Value {
                    decimals: d - b'0',
                    sign,
                }),
                _ => Err("decimals must be a single digit from 0 to 9"),
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
            Piece::Pace => match value.filter(|v| v.is_finite() && *v >= 0.0 && *v < 1e12) {
                Some(v) => {
                    let seconds = (v * 60.0).round() as u64;
                    let _ = write!(out, "{}:{:02}", seconds / 60, seconds % 60);
                }
                None => out.push_str(EMPTY),
            },
            Piece::Value { decimals, sign } => match value.filter(|v| v.is_finite()) {
                None => out.push_str(EMPTY),
                Some(v) => {
                    // written in place: no allocation per value (the renderer calls this
                    // for every widget of every frame)
                    let d = usize::from(*decimals);
                    let start = out.len();
                    let _ = if *sign {
                        write!(out, "{v:+.d$}")
                    } else {
                        write!(out, "{v:.d$}")
                    };
                    // a value that rounds to zero never prints "-0" / "-0.0"
                    if out[start..]
                        .chars()
                        .all(|c| matches!(c, '0' | '.' | '-' | '+'))
                    {
                        out.truncate(start);
                        let _ = if *sign {
                            write!(out, "{:+.d$}", 0.0)
                        } else {
                            write!(out, "{:.d$}", 0.0)
                        };
                    }
                }
            },
        }
    }
}
/// True when `fmt` is a valid strftime format (for `datetime` widgets). Validation
/// rejects invalid ones; the renderer shows the empty state for them.
pub fn is_valid_strftime(fmt: &str) -> bool {
    !chrono::format::StrftimeItems::new(fmt).any(|i| matches!(i, chrono::format::Item::Error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strftime_validity() {
        for ok in ["%H:%M:%S", "%a %d %b %Y", "", "plain %%"] {
            assert!(is_valid_strftime(ok), "{ok}");
        }
        for bad in ["%H:%Q", "%", "%E"] {
            assert!(!is_valid_strftime(bad), "{bad}");
        }
    }

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
    fn rounding_ties_follow_format_and_never_print_minus_zero() {
        assert_eq!(fmt("{value:.0}", Some(-0.5), ""), "0");
        assert_eq!(fmt("{value:+.0}", Some(-0.5), ""), "+0");
        assert_eq!(fmt("{value:.1}", Some(-0.04), ""), "0.0");
        assert_eq!(fmt("{value:.1}", Some(-0.25), ""), "-0.2");
        assert_eq!(fmt("{value:.0}", Some(2.5), ""), "2");
    }

    #[test]
    fn malformed_and_tricky_formats_table() {
        let lit = |s: &str| Piece::Literal(s.into());
        let val = Piece::Value {
            decimals: 0,
            sign: false,
        };
        let ok: Vec<(&str, Vec<Piece>)> = vec![
            ("{{{value}}}", vec![lit("{"), val.clone(), lit("}")]),
            (
                "{value:.9}",
                vec![Piece::Value {
                    decimals: 9,
                    sign: false,
                }],
            ),
        ];
        for (input, want) in ok {
            assert_eq!(parse(input).unwrap(), want, "{input}");
        }
        let err: Vec<(&str, usize)> = vec![
            ("{value", 0),
            ("ab{", 2),
            ("}", 0),
            ("a}", 1),
            ("{value:.1}}", 10),
            ("°{speed}", 2),
            ("è{value", 2),
            ("{value:.999999999}", 0),
            ("{value:.+1}", 0),
            ("{value:.09}", 0),
            ("{value:.}", 0),
            ("{}", 0),
            ("{:}", 0),
            ("{value::}", 0),
        ];
        for (input, at) in err {
            let e = parse(input).expect_err(input);
            assert_eq!(e.at, at, "{input}: {e}");
        }
    }

    #[test]
    fn error_positions_are_char_boundaries() {
        let alphabet = ['{', '}', 'è', '°', 'v', ':', '.', '+', '1'];
        for n in 0..6u32 {
            for seed in 0..alphabet.len().pow(n.min(4)) * 4 {
                let mut x = seed;
                let input: String = (0..n)
                    .map(|_| {
                        let c = alphabet[x % alphabet.len()];
                        x = x / alphabet.len() + 7;
                        c
                    })
                    .collect();
                if let Err(e) = parse(&input) {
                    assert!(
                        e.at < input.len() && input.is_char_boundary(e.at),
                        "{input}"
                    );
                }
            }
        }
    }
}
