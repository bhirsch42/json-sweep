use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone)]
pub enum Generator {
    List(Vec<Value>),
    RangeInt {
        start: i64,
        end: i64,
        inclusive: bool,
    },
    Stepped {
        start: f64,
        end: f64,
        step: f64,
        inclusive: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenError {
    pub message: String,
    pub offset: usize,
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "col {}: {}", self.offset + 1, self.message)
    }
}

impl std::error::Error for GenError {}

impl GenError {
    fn at(offset: usize, msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            offset,
        }
    }
}

impl Generator {
    pub fn expand(&self) -> Vec<Value> {
        match self {
            Generator::List(vs) => vs.clone(),
            Generator::RangeInt {
                start,
                end,
                inclusive,
            } => {
                let mut out = Vec::new();
                let last = if *inclusive { *end } else { *end - 1 };
                let mut i = *start;
                while i <= last {
                    out.push(Value::from(i));
                    i += 1;
                }
                out
            }
            Generator::Stepped {
                start,
                end,
                step,
                inclusive,
            } => {
                let mut out = Vec::new();
                if *step == 0.0 {
                    return out;
                }
                let mut i: usize = 0;
                loop {
                    let v = *start + (i as f64) * *step;
                    let past_end = if *step > 0.0 {
                        if *inclusive { v > *end } else { v >= *end }
                    } else if *inclusive {
                        v < *end
                    } else {
                        v <= *end
                    };
                    if past_end {
                        break;
                    }
                    out.push(json_num(v));
                    i += 1;
                    if i > 10_000_000 {
                        break;
                    }
                }
                out
            }
        }
    }
}

fn json_num(v: f64) -> Value {
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

pub fn parse_generator(input: &str) -> Result<Generator, GenError> {
    if input.is_empty() {
        return Err(GenError::at(0, "empty generator"));
    }
    if let Some(g) = try_range_form(input)? {
        return Ok(g);
    }
    let values = parse_list(input)?;
    Ok(Generator::List(values))
}

fn try_range_form(input: &str) -> Result<Option<Generator>, GenError> {
    let dotdot = find_top_level_dotdot(input)?;
    let Some(op_pos) = dotdot else {
        return Ok(None);
    };
    let lhs = &input[..op_pos];
    let (inclusive, after_op) = if input[op_pos..].starts_with("..=") {
        (true, op_pos + 3)
    } else {
        (false, op_pos + 2)
    };
    let rest = &input[after_op..];
    let (rhs, step_str) = match find_top_level_colon(rest) {
        Some(colon_idx) => (&rest[..colon_idx], Some(&rest[colon_idx + 1..])),
        None => (rest, None),
    };

    let lhs = lhs.trim();
    let rhs = rhs.trim();
    if lhs.is_empty() {
        return Err(GenError::at(0, "range missing start"));
    }
    if rhs.is_empty() {
        return Err(GenError::at(after_op, "range missing end"));
    }

    let rhs_offset = after_op;
    match step_str {
        None => {
            let start = parse_int(lhs, 0)?;
            let end = parse_int(rhs, rhs_offset)?;
            Ok(Some(Generator::RangeInt {
                start,
                end,
                inclusive,
            }))
        }
        Some(step_raw) => {
            let step_offset = after_op
                + rhs.len()
                + (rest.len() - rhs.len() - step_raw.len());
            let step_str = step_raw.trim();
            if step_str.is_empty() {
                return Err(GenError::at(step_offset, "stepped range missing step"));
            }
            let start = parse_float(lhs, 0)?;
            let end = parse_float(rhs, rhs_offset)?;
            let step = parse_float(step_str, step_offset)?;
            Ok(Some(Generator::Stepped {
                start,
                end,
                step,
                inclusive,
            }))
        }
    }
}

fn find_top_level_dotdot(s: &str) -> Result<Option<usize>, GenError> {
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth < 0 {
                    return Err(GenError::at(i, "unbalanced closing brace"));
                }
            }
            b'.' if depth == 0
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'.' =>
            {
                return Ok(Some(i));
            }
            _ => {}
        }
        i += 1;
    }
    if in_str {
        return Err(GenError::at(s.len(), "unterminated string"));
    }
    if depth != 0 {
        return Err(GenError::at(s.len(), "unbalanced braces"));
    }
    Ok(None)
}

fn find_top_level_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_int(s: &str, offset: usize) -> Result<i64, GenError> {
    s.parse::<i64>()
        .map_err(|_| GenError::at(offset, format!("expected integer, found {s:?}")))
}

fn parse_float(s: &str, offset: usize) -> Result<f64, GenError> {
    s.parse::<f64>()
        .map_err(|_| GenError::at(offset, format!("expected number, found {s:?}")))
}

fn parse_list(input: &str) -> Result<Vec<Value>, GenError> {
    let parts = split_top_level_commas(input)?;
    let mut out = Vec::with_capacity(parts.len());
    for (slice_offset, raw) in parts {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(GenError::at(slice_offset, "empty value (trailing comma?)"));
        }
        let leading_ws = raw.len() - raw.trim_start().len();
        let value_offset = slice_offset + leading_ws;
        out.push(parse_value(trimmed, value_offset)?);
    }
    Ok(out)
}

fn split_top_level_commas(input: &str) -> Result<Vec<(usize, &str)>, GenError> {
    let bytes = input.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut start = 0;
    let mut parts: Vec<(usize, &str)> = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth < 0 {
                    return Err(GenError::at(i, "unbalanced closing brace"));
                }
            }
            b',' if depth == 0 => {
                parts.push((start, &input[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    if in_str {
        return Err(GenError::at(input.len(), "unterminated string"));
    }
    if depth != 0 {
        return Err(GenError::at(input.len(), "unbalanced braces"));
    }
    parts.push((start, &input[start..]));
    Ok(parts)
}

fn parse_value(s: &str, offset: usize) -> Result<Value, GenError> {
    let first = s.chars().next().expect("non-empty");
    match first {
        '"' => serde_json::from_str(s).map_err(|e| {
            GenError::at(offset, format!("invalid JSON string: {e}"))
        }),
        '{' | '[' => serde_json::from_str(s).map_err(|e| {
            GenError::at(offset, format!("invalid JSON literal: {e}"))
        }),
        _ if s == "true" => Ok(Value::Bool(true)),
        _ if s == "false" => Ok(Value::Bool(false)),
        _ if s == "null" => Ok(Value::Null),
        '-' => parse_number_value(s, offset),
        c if c.is_ascii_digit() => parse_number_value(s, offset),
        c if c.is_ascii_alphabetic() || c == '_' || c == '/' || c == '.' => {
            Ok(Value::String(s.to_string()))
        }
        _ => Err(GenError::at(
            offset,
            format!("unrecognized value start {first:?}"),
        )),
    }
}

fn parse_number_value(s: &str, offset: usize) -> Result<Value, GenError> {
    serde_json::from_str::<Value>(s).map_err(|_| {
        GenError::at(offset, format!("expected number, found {s:?}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn build(input: &str) -> Generator {
        parse_generator(input).unwrap_or_else(|e| panic!("parse failed: {e} ({input:?})"))
    }

    fn expand(input: &str) -> Vec<Value> {
        build(input).expand()
    }

    fn err(input: &str) {
        assert!(
            parse_generator(input).is_err(),
            "expected error for {input:?}"
        );
    }

    // ---------- Lists ----------

    #[test]
    fn list_ints() {
        assert_eq!(expand("1,2,3"), vec![json!(1), json!(2), json!(3)]);
    }

    #[test]
    fn list_floats() {
        assert_eq!(
            expand("0.5,1.0,2.0"),
            vec![json!(0.5), json!(1.0), json!(2.0)]
        );
    }

    #[test]
    fn list_bools() {
        assert_eq!(expand("true,false"), vec![json!(true), json!(false)]);
    }

    #[test]
    fn list_bare_strings() {
        assert_eq!(
            expand("argmax,softmax"),
            vec![json!("argmax"), json!("softmax")]
        );
    }

    #[test]
    fn list_null() {
        assert_eq!(expand("null"), vec![Value::Null]);
    }

    #[test]
    fn list_quoted_strings() {
        assert_eq!(
            expand(r#""with spaces","also quoted""#),
            vec![json!("with spaces"), json!("also quoted")]
        );
    }

    #[test]
    fn list_json_objects() {
        assert_eq!(
            expand(r#"{"a":1},{"b":2}"#),
            vec![json!({"a": 1}), json!({"b": 2})]
        );
    }

    #[test]
    fn list_json_objects_with_nested_commas() {
        assert_eq!(
            expand(r#"{"a":1,"b":2},{"c":[1,2,3]}"#),
            vec![json!({"a": 1, "b": 2}), json!({"c": [1, 2, 3]})]
        );
    }

    #[test]
    fn list_single_bare_string_with_slash() {
        assert_eq!(expand("./variant.json"), vec![json!("./variant.json")]);
    }

    #[test]
    fn list_kebab_string() {
        assert_eq!(
            expand("kebab-case-thing"),
            vec![json!("kebab-case-thing")]
        );
    }

    // ---------- Ranges ----------

    #[test]
    fn range_half_open() {
        assert_eq!(expand("1..5"), vec![json!(1), json!(2), json!(3), json!(4)]);
    }

    #[test]
    fn range_inclusive() {
        assert_eq!(
            expand("1..=5"),
            vec![json!(1), json!(2), json!(3), json!(4), json!(5)]
        );
    }

    #[test]
    fn range_single_element_inclusive() {
        assert_eq!(expand("3..=3"), vec![json!(3)]);
    }

    #[test]
    fn range_empty_half_open() {
        assert_eq!(expand("5..5"), Vec::<Value>::new());
    }

    // ---------- Stepped ranges ----------

    #[test]
    fn stepped_inclusive() {
        assert_eq!(
            expand("0..=1:0.25"),
            vec![json!(0.0), json!(0.25), json!(0.5), json!(0.75), json!(1.0)]
        );
    }

    #[test]
    fn stepped_half_open() {
        assert_eq!(
            expand("0..1:0.25"),
            vec![json!(0.0), json!(0.25), json!(0.5), json!(0.75)]
        );
    }

    #[test]
    fn stepped_float_no_drift() {
        let vals = expand("0..=1:0.1");
        // Index 10 should be exactly 1.0, computed as 10 * 0.1, not 0.1 added ten times.
        assert_eq!(vals.last(), Some(&json!(1.0)));
        assert_eq!(vals.len(), 11);
        // index 10 = 10*0.1 = 1.0 exactly under our formula.
        let raw_val: f64 = vals.last().unwrap().as_f64().unwrap();
        assert_eq!(raw_val, 10.0 * 0.1);
        assert_eq!(raw_val, 1.0);
    }

    // ---------- Rejects ----------

    #[test]
    fn rejects_empty() {
        err("");
    }

    #[test]
    fn rejects_trailing_comma() {
        err("1,2,");
    }

    #[test]
    fn rejects_leading_comma() {
        err(",1,2");
    }

    #[test]
    fn rejects_unbalanced_braces() {
        err(r#"{"a":1"#);
        err(r#"{"a":1}}"#);
    }

    #[test]
    fn rejects_non_integer_in_range() {
        err("1..=true");
        err("1.0..5");
        err("1..=2.5");
    }

    #[test]
    fn rejects_missing_range_endpoint() {
        err("..5");
        err("1..");
    }

    #[test]
    fn rejects_missing_step() {
        err("0..=1:");
    }
}
