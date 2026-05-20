use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Key(String),
    Index(usize),
    Filter { key: String, value: Value },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SegmentTemplate {
    Key(String),
    Index(usize),
    IndexRange {
        start: i64,
        end: i64,
        inclusive: bool,
    },
    Filter {
        key: String,
        value: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathError {
    pub message: String,
    pub offset: usize,
}

impl PathError {
    fn at(offset: usize, msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            offset,
        }
    }
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "col {}: {}", self.offset + 1, self.message)
    }
}

impl std::error::Error for PathError {}

/// Parse a path into a list of segment templates.
///
/// Grammar:
///   path    = head ( "." key | bracket )*
///   head    = key | bracket
///   key     = [A-Za-z_] [A-Za-z0-9_-]*
///   bracket = "[" ( index_or_range | filter ) "]"
///   index_or_range = uint ( ".." "="? uint )?
///   filter         = key "=" value_token
pub fn parse_path(input: &str) -> Result<Vec<SegmentTemplate>, PathError> {
    if input.is_empty() {
        return Err(PathError::at(0, "empty path"));
    }
    let mut p = Parser {
        input,
        pos: 0,
        out: Vec::new(),
    };
    p.parse_head()?;
    while p.pos < input.len() {
        match input.as_bytes()[p.pos] {
            b'.' => {
                p.pos += 1;
                let seg = p.parse_key_segment()?;
                p.out.push(seg);
            }
            b'[' => {
                let seg = p.parse_bracket()?;
                p.out.push(seg);
            }
            other => {
                return Err(PathError::at(
                    p.pos,
                    format!("unexpected character {:?}", other as char),
                ));
            }
        }
    }
    Ok(p.out)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    out: Vec<SegmentTemplate>,
}

impl<'a> Parser<'a> {
    fn parse_head(&mut self) -> Result<(), PathError> {
        match self.input.as_bytes().first() {
            Some(b'[') => {
                let seg = self.parse_bracket()?;
                self.out.push(seg);
                Ok(())
            }
            Some(&b) if is_ident_start(b) => {
                let seg = self.parse_key_segment()?;
                self.out.push(seg);
                Ok(())
            }
            Some(&b) => Err(PathError::at(
                0,
                format!("path must start with a key or '[', found {:?}", b as char),
            )),
            None => Err(PathError::at(0, "empty path")),
        }
    }

    fn parse_key_segment(&mut self) -> Result<SegmentTemplate, PathError> {
        let bytes = self.input.as_bytes();
        let start = self.pos;
        if start >= bytes.len() || !is_ident_start(bytes[start]) {
            return Err(PathError::at(start, "expected key"));
        }
        let mut end = start + 1;
        while end < bytes.len() && is_ident_continue(bytes[end]) {
            end += 1;
        }
        let key = self.input[start..end].to_string();
        self.pos = end;
        Ok(SegmentTemplate::Key(key))
    }

    fn parse_bracket(&mut self) -> Result<SegmentTemplate, PathError> {
        let open = self.pos;
        debug_assert_eq!(self.input.as_bytes()[open], b'[');
        self.pos += 1;
        let body_start = self.pos;
        let close = find_close_bracket(self.input, body_start)
            .ok_or_else(|| PathError::at(open, "unterminated '['"))?;
        let body = &self.input[body_start..close];
        let body_trimmed = body.trim();
        if body_trimmed.is_empty() {
            return Err(PathError::at(body_start, "empty brackets"));
        }
        let seg = self.parse_bracket_body(body, body_start)?;
        self.pos = close + 1;
        Ok(seg)
    }

    fn parse_bracket_body(
        &self,
        body: &str,
        body_offset: usize,
    ) -> Result<SegmentTemplate, PathError> {
        // Check for `..` first so that `0..=2` (range) isn't mis-detected as a
        // filter due to the `=` in `..=`.
        if let Some(op_local) = find_dotdot(body) {
            let lhs = body[..op_local].trim();
            let (inclusive, after_op) = if body[op_local..].starts_with("..=") {
                (true, op_local + 3)
            } else {
                (false, op_local + 2)
            };
            let rhs = body[after_op..].trim();
            if lhs.is_empty() {
                return Err(PathError::at(body_offset, "range missing start"));
            }
            if rhs.is_empty() {
                return Err(PathError::at(body_offset + after_op, "range missing end"));
            }
            let start_leading = body[..op_local].len() - body[..op_local].trim_start().len();
            let start = parse_int(lhs, body_offset + start_leading)?;
            let rhs_leading = body[after_op..].len() - body[after_op..].trim_start().len();
            let end = parse_int(rhs, body_offset + after_op + rhs_leading)?;
            if start < 0 {
                return Err(PathError::at(
                    body_offset + start_leading,
                    format!("array index must be non-negative, found {start}"),
                ));
            }
            if end < 0 {
                return Err(PathError::at(
                    body_offset + after_op + rhs_leading,
                    format!("array index must be non-negative, found {end}"),
                ));
            }
            return Ok(SegmentTemplate::IndexRange {
                start,
                end,
                inclusive,
            });
        }

        if let Some(eq_local) = find_top_level_eq(body) {
            let key_str = body[..eq_local].trim();
            let val_str = body[eq_local + 1..].trim();
            if key_str.is_empty() {
                return Err(PathError::at(body_offset, "filter is missing key"));
            }
            if !is_valid_ident(key_str) {
                let leading = body[..eq_local].len() - body[..eq_local].trim_start().len();
                return Err(PathError::at(
                    body_offset + leading,
                    format!("filter key must be an identifier, found {key_str:?}"),
                ));
            }
            if val_str.is_empty() {
                return Err(PathError::at(
                    body_offset + eq_local + 1,
                    "filter is missing value",
                ));
            }
            let val_leading_ws =
                body[eq_local + 1..].len() - body[eq_local + 1..].trim_start().len();
            let value = parse_filter_value(val_str, body_offset + eq_local + 1 + val_leading_ws)?;
            return Ok(SegmentTemplate::Filter {
                key: key_str.to_string(),
                value,
            });
        }

        let trimmed = body.trim();
        let leading = body.len() - body.trim_start().len();
        let n = parse_int(trimmed, body_offset + leading)?;
        if n < 0 {
            return Err(PathError::at(
                body_offset + leading,
                format!("array index must be non-negative, found {n}"),
            ));
        }
        Ok(SegmentTemplate::Index(n as usize))
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn is_valid_ident(s: &str) -> bool {
    let bytes = s.as_bytes();
    !bytes.is_empty()
        && is_ident_start(bytes[0])
        && bytes[1..].iter().all(|&b| is_ident_continue(b))
}

fn find_close_bracket(input: &str, from: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut i = from;
    let mut in_str = false;
    let mut esc = false;
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
        } else {
            match b {
                b'"' => in_str = true,
                b']' => return Some(i),
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn find_top_level_eq(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
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
        } else {
            match b {
                b'"' => in_str = true,
                b'=' => return Some(i),
                _ => {}
            }
        }
    }
    None
}

fn find_dotdot(s: &str) -> Option<usize> {
    s.as_bytes()
        .windows(2)
        .position(|w| w == b"..")
}

fn parse_int(s: &str, offset: usize) -> Result<i64, PathError> {
    s.parse::<i64>()
        .map_err(|_| PathError::at(offset, format!("expected integer, found {s:?}")))
}

/// Parse a filter value: number, bool, null, quoted string, or bare string.
fn parse_filter_value(s: &str, offset: usize) -> Result<Value, PathError> {
    if s == "true" {
        return Ok(Value::Bool(true));
    }
    if s == "false" {
        return Ok(Value::Bool(false));
    }
    if s == "null" {
        return Ok(Value::Null);
    }
    let first = s.as_bytes()[0];
    if first == b'"' {
        return serde_json::from_str::<Value>(s)
            .map_err(|e| PathError::at(offset, format!("invalid JSON string: {e}")));
    }
    if first == b'-' || first.is_ascii_digit() {
        if let Ok(v) = serde_json::from_str::<Value>(s)
            && v.is_number()
        {
            return Ok(v);
        }
        return Err(PathError::at(offset, format!("expected number, found {s:?}")));
    }
    // Bare string: any chars that aren't whitespace, '[', ']', or '='. The
    // outer scanner already stripped surrounding whitespace.
    if s.bytes().any(|b| b == b'[' || b == b']') {
        return Err(PathError::at(
            offset,
            format!("unexpected bracket in filter value {s:?}"),
        ));
    }
    Ok(Value::String(s.to_string()))
}

/// Expand a path template into one or more concrete segment lists by
/// enumerating every IndexRange in cartesian order (leftmost varies slowest).
pub fn expand_path(template: &[SegmentTemplate]) -> Vec<Vec<Segment>> {
    let mut out: Vec<Vec<Segment>> = vec![Vec::with_capacity(template.len())];
    for tmpl in template {
        match tmpl {
            SegmentTemplate::Key(k) => {
                for path in &mut out {
                    path.push(Segment::Key(k.clone()));
                }
            }
            SegmentTemplate::Index(i) => {
                for path in &mut out {
                    path.push(Segment::Index(*i));
                }
            }
            SegmentTemplate::Filter { key, value } => {
                for path in &mut out {
                    path.push(Segment::Filter {
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }
            SegmentTemplate::IndexRange {
                start,
                end,
                inclusive,
            } => {
                let last = if *inclusive { *end } else { *end - 1 };
                let mut new_out = Vec::new();
                for path in &out {
                    let mut i = *start;
                    while i <= last {
                        let mut copy = path.clone();
                        copy.push(Segment::Index(i as usize));
                        new_out.push(copy);
                        i += 1;
                    }
                }
                out = new_out;
            }
        }
    }
    out
}

/// Render a concrete path back into source-style notation, e.g.
/// `classes[5].weight` or `classes[name=Treasury].weight`.
pub fn segments_to_string(segs: &[Segment]) -> String {
    let mut s = String::new();
    for (i, seg) in segs.iter().enumerate() {
        match seg {
            Segment::Key(k) => {
                if i > 0 {
                    s.push('.');
                }
                s.push_str(k);
            }
            Segment::Index(idx) => {
                s.push('[');
                s.push_str(&idx.to_string());
                s.push(']');
            }
            Segment::Filter { key, value } => {
                s.push('[');
                s.push_str(key);
                s.push('=');
                match value {
                    Value::String(v) => s.push_str(v),
                    other => s.push_str(&other.to_string()),
                }
                s.push(']');
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(input: &str) -> Vec<SegmentTemplate> {
        parse_path(input).unwrap_or_else(|e| panic!("parse failed: {e} ({input:?})"))
    }

    fn err(input: &str) {
        assert!(parse_path(input).is_err(), "expected error for {input:?}");
    }

    #[test]
    fn single_key() {
        assert_eq!(parse("a"), vec![SegmentTemplate::Key("a".into())]);
        assert_eq!(
            parse("kebab-case"),
            vec![SegmentTemplate::Key("kebab-case".into())]
        );
    }

    #[test]
    fn dotted_keys() {
        assert_eq!(
            parse("a.b.c"),
            vec![
                SegmentTemplate::Key("a".into()),
                SegmentTemplate::Key("b".into()),
                SegmentTemplate::Key("c".into())
            ]
        );
    }

    #[test]
    fn numeric_index() {
        assert_eq!(
            parse("classes[5]"),
            vec![
                SegmentTemplate::Key("classes".into()),
                SegmentTemplate::Index(5)
            ]
        );
    }

    #[test]
    fn index_then_key() {
        assert_eq!(
            parse("classes[5].weight"),
            vec![
                SegmentTemplate::Key("classes".into()),
                SegmentTemplate::Index(5),
                SegmentTemplate::Key("weight".into()),
            ]
        );
    }

    #[test]
    fn chained_indices() {
        assert_eq!(
            parse("a[0][1][2]"),
            vec![
                SegmentTemplate::Key("a".into()),
                SegmentTemplate::Index(0),
                SegmentTemplate::Index(1),
                SegmentTemplate::Index(2),
            ]
        );
    }

    #[test]
    fn head_can_be_bracket() {
        assert_eq!(
            parse("[3].name"),
            vec![
                SegmentTemplate::Index(3),
                SegmentTemplate::Key("name".into())
            ]
        );
    }

    #[test]
    fn filter_bare_string() {
        assert_eq!(
            parse("classes[name=Treasury].ideal"),
            vec![
                SegmentTemplate::Key("classes".into()),
                SegmentTemplate::Filter {
                    key: "name".into(),
                    value: json!("Treasury"),
                },
                SegmentTemplate::Key("ideal".into()),
            ]
        );
    }

    #[test]
    fn filter_kebab_value() {
        assert_eq!(
            parse("classes[id=t-1]"),
            vec![
                SegmentTemplate::Key("classes".into()),
                SegmentTemplate::Filter {
                    key: "id".into(),
                    value: json!("t-1"),
                },
            ]
        );
    }

    #[test]
    fn filter_int_value() {
        assert_eq!(
            parse("xs[id=42]"),
            vec![
                SegmentTemplate::Key("xs".into()),
                SegmentTemplate::Filter {
                    key: "id".into(),
                    value: json!(42),
                },
            ]
        );
    }

    #[test]
    fn filter_quoted_string() {
        assert_eq!(
            parse(r#"xs[name="with space"]"#),
            vec![
                SegmentTemplate::Key("xs".into()),
                SegmentTemplate::Filter {
                    key: "name".into(),
                    value: json!("with space"),
                },
            ]
        );
    }

    #[test]
    fn filter_bool_and_null() {
        assert_eq!(
            parse("xs[enabled=true]"),
            vec![
                SegmentTemplate::Key("xs".into()),
                SegmentTemplate::Filter {
                    key: "enabled".into(),
                    value: json!(true),
                },
            ]
        );
        assert_eq!(
            parse("xs[parent=null]"),
            vec![
                SegmentTemplate::Key("xs".into()),
                SegmentTemplate::Filter {
                    key: "parent".into(),
                    value: json!(null),
                },
            ]
        );
    }

    #[test]
    fn range_inclusive_and_half_open() {
        assert_eq!(
            parse("classes[0..=2]"),
            vec![
                SegmentTemplate::Key("classes".into()),
                SegmentTemplate::IndexRange {
                    start: 0,
                    end: 2,
                    inclusive: true,
                },
            ]
        );
        assert_eq!(
            parse("classes[0..3]"),
            vec![
                SegmentTemplate::Key("classes".into()),
                SegmentTemplate::IndexRange {
                    start: 0,
                    end: 3,
                    inclusive: false,
                },
            ]
        );
    }

    #[test]
    fn rejects_empty() {
        err("");
    }

    #[test]
    fn rejects_leading_dot() {
        err(".foo");
    }

    #[test]
    fn rejects_trailing_dot() {
        err("foo.");
    }

    #[test]
    fn rejects_double_dot() {
        err("foo..bar");
    }

    #[test]
    fn rejects_unterminated_bracket() {
        err("foo[5");
    }

    #[test]
    fn rejects_empty_brackets() {
        err("foo[]");
    }

    #[test]
    fn rejects_negative_index() {
        err("classes[-1]");
    }

    #[test]
    fn rejects_non_integer_in_range() {
        err("classes[1..=foo]");
    }

    #[test]
    fn rejects_filter_with_bad_key() {
        err("classes[1=foo]");
    }

    // ---------- expand_path ----------

    #[test]
    fn expand_no_ranges_returns_one_path() {
        let t = parse("a.b[5].c");
        let exp = expand_path(&t);
        assert_eq!(exp.len(), 1);
        assert_eq!(
            exp[0],
            vec![
                Segment::Key("a".into()),
                Segment::Key("b".into()),
                Segment::Index(5),
                Segment::Key("c".into()),
            ]
        );
    }

    #[test]
    fn expand_single_range() {
        let t = parse("classes[0..=2].weight");
        let exp = expand_path(&t);
        assert_eq!(exp.len(), 3);
        let last_idx_for_each: Vec<usize> = exp
            .iter()
            .map(|segs| match segs[1] {
                Segment::Index(i) => i,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(last_idx_for_each, vec![0, 1, 2]);
    }

    #[test]
    fn expand_two_ranges_cartesian() {
        let t = parse("a[0..=1].b[0..=1]");
        let exp = expand_path(&t);
        // 2 × 2 = 4, leftmost varies slowest.
        assert_eq!(exp.len(), 4);
        let idx_pairs: Vec<(usize, usize)> = exp
            .iter()
            .map(|segs| match (&segs[1], &segs[3]) {
                (Segment::Index(i), Segment::Index(j)) => (*i, *j),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(idx_pairs, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn segments_to_string_roundtrips_for_simple_paths() {
        let segs = vec![
            Segment::Key("classes".into()),
            Segment::Index(5),
            Segment::Key("weight".into()),
        ];
        assert_eq!(segments_to_string(&segs), "classes[5].weight");

        let segs = vec![
            Segment::Key("classes".into()),
            Segment::Filter {
                key: "name".into(),
                value: json!("Treasury"),
            },
            Segment::Key("ideal".into()),
        ];
        assert_eq!(segments_to_string(&segs), "classes[name=Treasury].ideal");
    }
}
