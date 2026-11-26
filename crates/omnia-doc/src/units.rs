//! Shared PK4 numeric grammar for schematic parameter values.

use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};

/// A finite real with a single representation for zero.
#[derive(Clone, Copy, Debug)]
pub struct FiniteF64(f64);

impl FiniteF64 {
    pub fn new(value: f64) -> Result<Self, ValueError> {
        if !value.is_finite() {
            return Err(ValueError::NonFinite);
        }
        Ok(Self(if value == 0.0 { 0.0 } else { value }))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

impl PartialEq for FiniteF64 {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for FiniteF64 {}

impl PartialOrd for FiniteF64 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FiniteF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Hash for FiniteF64 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueError {
    InvalidNumber,
    InvalidWhitespace,
    InvalidSuffix,
    NonFinite,
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidNumber => "expected a decimal number",
            Self::InvalidWhitespace => "only one ASCII space may separate the number and suffix",
            Self::InvalidSuffix => "suffix does not match the declared unit",
            Self::NonFinite => "value must be finite",
        })
    }
}

impl std::error::Error for ValueError {}

/// Parses a decimal value with an optional engineering prefix and declared unit (PK4, PQ2).
pub fn parse_value(text: &str, unit: Option<&str>) -> Result<FiniteF64, ValueError> {
    let text = text.trim();
    let (end, exp_index) = decimal_parts(text)?;
    let (number, suffix) = text.split_at(end);
    let suffix = suffix.strip_prefix(' ').unwrap_or(suffix);
    if suffix.chars().any(char::is_whitespace) {
        return Err(ValueError::InvalidWhitespace);
    }
    let extra_exp = suffix_exponent(suffix, unit)?;
    let value = if extra_exp == 0 {
        number
            .parse::<f64>()
            .map_err(|_| ValueError::InvalidNumber)?
    } else {
        let (mantissa, base_exp) = match exp_index {
            Some(idx) => (&number[..idx], parse_exp_saturating(&number[idx + 1..])),
            None => (number, 0),
        };
        let combined = base_exp.saturating_add(i64::from(extra_exp));
        format!("{mantissa}e{combined}")
            .parse::<f64>()
            .map_err(|_| ValueError::InvalidNumber)?
    };
    FiniteF64::new(value)
}

fn decimal_parts(text: &str) -> Result<(usize, Option<usize>), ValueError> {
    let bytes = text.as_bytes();
    let sign_end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer_end = digits_end(bytes, sign_end);
    let mut end = integer_end;
    let mut has_digits = integer_end > sign_end;
    if bytes.get(end) == Some(&b'.') {
        end = digits_end(bytes, end + 1);
        has_digits |= end > integer_end + 1;
    }
    if !has_digits {
        return Err(ValueError::InvalidNumber);
    }
    let mut exp_index = None;
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent = end + 1;
        exponent += usize::from(matches!(bytes.get(exponent), Some(b'+' | b'-')));
        let exponent_end = digits_end(bytes, exponent);
        if exponent_end > exponent {
            exp_index = Some(end);
            end = exponent_end;
        }
    }
    Ok((end, exp_index))
}

fn parse_exp_saturating(exp_text: &str) -> i64 {
    let bytes = exp_text.as_bytes();
    let (neg, digits) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        Some(b'+') => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    let mut acc = 0i64;
    for &b in digits {
        let digit = i64::from(b - b'0');
        acc = if neg {
            acc.saturating_mul(10).saturating_sub(digit)
        } else {
            acc.saturating_mul(10).saturating_add(digit)
        };
    }
    acc
}

fn digits_end(bytes: &[u8], start: usize) -> usize {
    start
        + bytes[start..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
}

fn suffix_exponent(suffix: &str, unit: Option<&str>) -> Result<i32, ValueError> {
    const PREFIXES: [(&str, i32); 14] = [
        ("meg", 6),
        ("MEG", 6),
        ("f", -15),
        ("p", -12),
        ("n", -9),
        ("u", -6),
        ("µ", -6),
        ("μ", -6),
        ("m", -3),
        ("k", 3),
        ("K", 3),
        ("M", 6),
        ("G", 9),
        ("T", 12),
    ];

    if suffix == "%" && unit == Some("ratio") {
        return Ok(-2);
    }
    if unit_matches(suffix, unit) {
        return Ok(0);
    }
    PREFIXES
        .into_iter()
        .find_map(|(prefix, exp)| {
            suffix
                .strip_prefix(prefix)
                .filter(|rest| unit_matches(rest, unit))
                .map(|_| exp)
        })
        .ok_or(ValueError::InvalidSuffix)
}

fn unit_matches(token: &str, unit: Option<&str>) -> bool {
    match (token, unit) {
        ("", _) => true,
        ("%", _) | (_, Some("ratio")) => false,
        ("Ω" | "ohm", Some("Ω" | "ohm")) => true,
        _ => Some(token) == unit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::hash::DefaultHasher;

    #[test]
    fn pk4_pq2_suffixes_and_decimal_grammar() {
        for (text, unit, expected) in [
            ("10M", Some("ohm"), 1e7),
            ("10m", Some("ohm"), 0.01),
            ("10meg", Some("ohm"), 1e7),
            ("10MEGohm", Some("ohm"), 1e7),
            ("\u{2003}10 kΩ\t", Some("ohm"), 1e4),
            ("10 Kohm", Some("ohm"), 1e4),
            ("1Ω", Some("ohm"), 1.0),
            ("1ohm", Some("Ω"), 1.0),
            ("4.7uF", Some("F"), 4.7e-6),
            ("1F", Some("F"), 1.0),
            ("10fF", Some("F"), 1e-14),
            ("10f", None, 1e-14),
            ("50%", Some("ratio"), 0.5),
            ("50 %", Some("ratio"), 0.5),
            ("+.5pH", Some("H"), 5e-13),
            ("-1e-3nA", Some("A"), -1e-12),
            ("1E+6µV", Some("V"), 1.0),
            ("1μs", Some("s"), 1e-6),
            ("2GHz", Some("Hz"), 2e9),
            ("10.T", None, 1e13),
            ("2.5 1/K", Some("1/K"), 2.5),
        ] {
            assert_eq!(parse_value(text, unit).unwrap().get(), expected, "{text}");
        }
    }

    #[test]
    fn pk4_single_rounding_matches_exponent_and_handles_ratio_and_saturating_exp() {
        for (prefixed, exp_form, unit) in [
            ("100n", "100e-9", None),
            ("10fF", "10e-15", Some("F")),
            ("2.2p", "2.2e-12", None),
            ("6.8nF", "6.8e-9", Some("F")),
            ("4.7uF", "4.7e-6", Some("F")),
            ("50%", "50e-2", Some("ratio")),
            ("50 %", "50e-2", Some("ratio")),
        ] {
            assert_eq!(
                parse_value(prefixed, unit).unwrap(),
                parse_value(exp_form, None).unwrap(),
                "{prefixed} vs {exp_form}"
            );
        }

        assert_eq!(
            parse_value("1e-999999999999999999999T", None)
                .unwrap()
                .get(),
            0.0
        );
        assert_eq!(
            parse_value("-1e-999999999999999999999T", None)
                .unwrap()
                .get()
                .to_bits(),
            0
        );
        assert_eq!(
            parse_value("1e999999999999999999999f", None),
            Err(ValueError::NonFinite)
        );
    }

    #[test]
    fn pk4_refuses_invalid_values() {
        for (text, unit) in [
            ("1F", Some("ohm")),
            ("1V", None),
            ("2v", Some("V")),
            ("4k7", Some("ohm")),
            ("1Meg", None),
            ("1meG", None),
            ("1 V A", Some("V A")),
            ("10\tkΩ", Some("ohm")),
            ("10  kΩ", Some("ohm")),
            ("10 k Ω", Some("ohm")),
            ("10\u{a0}kΩ", Some("ohm")),
            ("1k%", Some("ratio")),
            ("0.5ratio", Some("ratio")),
            ("50mratio", Some("ratio")),
            ("50%", None),
            ("50%", Some("V")),
            ("50%", Some("%")),
            ("NaN", None),
            ("-inf", None),
            ("+Inf", None),
            ("1e309", None),
            ("1e308T", None),
            ("1e-", None),
            ("1e", None),
            (".", None),
            ("+", None),
            ("", None),
            ("0x10", None),
            ("1_000", None),
        ] {
            assert!(parse_value(text, unit).is_err(), "{text}");
        }
    }

    #[test]
    fn pd6_canonical_zero_and_non_finite_values() {
        let negative = parse_value("-0.0", None).unwrap();
        let positive = FiniteF64::new(0.0).unwrap();
        assert_eq!(negative.get().to_bits(), 0);
        assert_eq!(negative, positive);
        assert_eq!(negative.cmp(&positive), Ordering::Equal);
        let hash = |value: FiniteF64| {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash(negative), hash(positive));
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(FiniteF64::new(value), Err(ValueError::NonFinite));
        }
    }

    proptest! {
        #[test]
        fn pd6_finite_value_equality_and_order(a in any::<u64>(), b in any::<u64>()) {
            let (a, b) = (f64::from_bits(a), f64::from_bits(b));
            match (FiniteF64::new(a), FiniteF64::new(b)) {
                (Ok(x), Ok(y)) => {
                    prop_assert_eq!(x == y, a == b);
                    prop_assert_eq!(x.partial_cmp(&y), a.partial_cmp(&b));
                    prop_assert_eq!(x.get().to_bits(), if a == 0.0 { 0 } else { a.to_bits() });
                }
                _ => prop_assert!(!a.is_finite() || !b.is_finite()),
            }
        }

        #[test]
        fn pk4_decimal_exponent_and_prefix_agree(
            neg in any::<bool>(),
            whole in 0u32..10_000,
            frac in 0u32..10_000,
            has_frac in any::<bool>(),
            base_exp in -20i32..=20,
            with_base_exp in any::<bool>(),
            with_space in any::<bool>(),
        ) {
            let sign = if neg { "-" } else { "" };
            let mantissa = if has_frac {
                format!("{sign}{whole}.{frac:04}")
            } else {
                format!("{sign}{whole}")
            };
            let decimal = if with_base_exp {
                format!("{mantissa}e{base_exp}")
            } else {
                mantissa.clone()
            };
            let space = if with_space { " " } else { "" };
            for (prefix, exp) in [
                ("f", -15),
                ("p", -12),
                ("n", -9),
                ("u", -6),
                ("µ", -6),
                ("μ", -6),
                ("m", -3),
                ("k", 3),
                ("K", 3),
                ("M", 6),
                ("meg", 6),
                ("MEG", 6),
                ("G", 9),
                ("T", 12),
            ] {
                let total_exp = if with_base_exp { base_exp + exp } else { exp };
                let parsed_prefixed = parse_value(&format!("{decimal}{space}{prefix}"), None).unwrap();
                let parsed_exp = parse_value(&format!("{mantissa}e{total_exp}"), None).unwrap();
                prop_assert_eq!(parsed_prefixed, parsed_exp, "prefix {}", prefix);
            }
            let total_ratio_exp = if with_base_exp { base_exp - 2 } else { -2 };
            let parsed_pct = parse_value(&format!("{decimal}{space}%"), Some("ratio")).unwrap();
            let parsed_exp = parse_value(&format!("{mantissa}e{total_ratio_exp}"), None).unwrap();
            prop_assert_eq!(parsed_pct, parsed_exp);
        }
    }
}
