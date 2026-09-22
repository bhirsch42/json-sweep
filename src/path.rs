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
    /// `.{k1,k2,...}` — fans the same swept value to multiple coupled keys
    /// within one axis. Distinct from `IndexRange`, which fans out to
    /// multiple *independent* axes.
    KeyGroup(Vec<String>),
}

/// Result of expanding one path template into the set of axes (and, within
/// each axis, the set of coupled paths) it represents.
///
/// - A vanilla `a.b.c` template produces one expansion with one path.
/// - A bracket range `a[0..=2].x` produces three expansions, each with one
///   path (`a[0].x`, `a[1].x`, `a[2].x`) — independent axes.
/// - A key group `a.{x,y}` produces one expansion with two paths
///   (`a.x`, `a.y`) — same axis, coupled.
/// - Both combine cartesian: `a[0..=1].{x,y}` is two expansions of two
///   paths each.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisExpansion {
    pub paths: Vec<Vec<Segment>>,
    /// Display label for this axis, rendered back from the template with
    /// range positions substituted but key-groups left as `{a,b}`.
    pub label: String,
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
        cur: Cursor::new(input),
        out: Vec::new(),
    };
    p.parse_head()?;
    while let Some(b) = p.cur.peek() {
        match b {
            b'.' => {
                p.cur.bump();
                let seg = match p.cur.peek() {
                    Some(b'{') => p.parse_key_group()?,
                    _ => p.parse_key_segment()?,
                };
                p.out.push(seg);
            }
            b'[' => {
                let seg = p.parse_bracket()?;
                p.out.push(seg);
            }
            other => {
                return Err(PathError::at(
                    p.cur.pos(),
                    format!("unexpected character {:?}", other as char),
                ));
            }
        }
    }
    Ok(p.out)
}

/// Minimal forward-only cursor over a `&str`. Tracks byte offset and exposes
/// the byte-level primitives the path parser needs (peek / bump / eat_while)
/// while leaving the lookahead-style helpers — `find_close_bracket`,
/// `find_top_level_eq` — as free functions that operate on the borrowed input.
struct Cursor<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn pos(&self) -> usize {
        self.pos
    }

    fn input(&self) -> &'a str {
        self.input
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn eat_while(&mut self, mut pred: impl FnMut(u8) -> bool) -> &'a str {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if !pred(b) {
                break;
            }
            self.pos += 1;
        }
        &self.input[start..self.pos]
    }

    /// Jump to an absolute byte offset. Must be forward and within bounds —
    /// used after a lookahead scan (e.g. `find_close_bracket`) returns the
    /// position to resume from.
    fn seek_to(&mut self, offset: usize) {
        debug_assert!(offset >= self.pos, "Cursor::seek_to went backwards");
        debug_assert!(offset <= self.input.len(), "Cursor::seek_to past end");
        self.pos = offset;
    }
}

struct Parser<'a> {
    cur: Cursor<'a>,
    out: Vec<SegmentTemplate>,
}

impl<'a> Parser<'a> {
    fn parse_head(&mut self) -> Result<(), PathError> {
        match self.cur.peek() {
            Some(b'[') => {
                let seg = self.parse_bracket()?;
                self.out.push(seg);
                Ok(())
            }
            Some(b) if is_ident_start(b) => {
                let seg = self.parse_key_segment()?;
                self.out.push(seg);
                Ok(())
            }
            Some(b) => Err(PathError::at(
                0,
                format!("path must start with a key or '[', found {:?}", b as char),
            )),
            None => Err(PathError::at(0, "empty path")),
        }
    }

    fn parse_key_segment(&mut self) -> Result<SegmentTemplate, PathError> {
        let start = self.cur.pos();
        match self.cur.peek() {
            Some(b) if is_ident_start(b) => {
                self.cur.bump();
            }
            _ => return Err(PathError::at(start, "expected key")),
        }
        self.cur.eat_while(is_ident_continue);
        let key = self.cur.input()[start..self.cur.pos()].to_string();
        Ok(SegmentTemplate::Key(key))
    }

    fn parse_bracket(&mut self) -> Result<SegmentTemplate, PathError> {
        let open = self.cur.pos();
        debug_assert_eq!(self.cur.peek(), Some(b'['));
        self.cur.bump();
        let body_start = self.cur.pos();
        let close = find_close_bracket(self.cur.input(), body_start)
            .ok_or_else(|| PathError::at(open, "unterminated '['"))?;
        let body = &self.cur.input()[body_start..close];
        if body.trim().is_empty() {
            return Err(PathError::at(body_start, "empty brackets"));
        }
        let seg = parse_bracket_body(body, body_start)?;
        self.cur.seek_to(close + 1);
        Ok(seg)
    }

    /// Parse `{key1,key2,...}` after we've just consumed the leading `.`.
    /// The cursor must be sitting on the `{`.
    fn parse_key_group(&mut self) -> Result<SegmentTemplate, PathError> {
        let open = self.cur.pos();
        debug_assert_eq!(self.cur.peek(), Some(b'{'));
        self.cur.bump();
        let body_start = self.cur.pos();
        let close = find_close_brace(self.cur.input(), body_start)
            .ok_or_else(|| PathError::at(open, "unterminated '{'"))?;
        let body = &self.cur.input()[body_start..close];
        let mut keys: Vec<String> = Vec::new();
        for (slice_offset, raw) in split_commas(body) {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(PathError::at(
                    body_start + slice_offset,
                    "empty key in {...} group",
                ));
            }
            if !is_valid_ident(trimmed) {
                let leading = raw.len() - raw.trim_start().len();
                return Err(PathError::at(
                    body_start + slice_offset + leading,
                    format!("key-group entry must be an identifier, found {trimmed:?}"),
                ));
            }
            keys.push(trimmed.to_string());
        }
        if keys.is_empty() {
            return Err(PathError::at(body_start, "empty key group"));
        }
        self.cur.seek_to(close + 1);
        Ok(SegmentTemplate::KeyGroup(keys))
    }
}

fn parse_bracket_body(body: &str, body_offset: usize) -> Result<SegmentTemplate, PathError> {
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
        let val_leading_ws = body[eq_local + 1..].len() - body[eq_local + 1..].trim_start().len();
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

fn find_close_brace(input: &str, from: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Split a brace-group body on top-level commas. The body holds bare
/// identifiers, so we don't need to skip quoted strings or nested
/// brackets — but we still avoid splitting if a comma appears at depth.
fn split_commas(body: &str) -> Vec<(usize, &str)> {
    let bytes = body.as_bytes();
    let mut parts: Vec<(usize, &str)> = Vec::new();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b',' {
            parts.push((start, &body[start..i]));
            start = i + 1;
        }
    }
    parts.push((start, &body[start..]));
    parts
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
    s.as_bytes().windows(2).position(|w| w == b"..")
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
        return Err(PathError::at(
            offset,
            format!("expected number, found {s:?}"),
        ));
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

/// Expand a path template into the axes (and per-axis coupled paths) it
/// represents. See `AxisExpansion` for the semantic split between
/// `IndexRange` (independent axes) and `KeyGroup` (coupled paths).
///
/// `IndexRange`s in cartesian order with leftmost varying slowest.
pub fn expand_template(template: &[SegmentTemplate]) -> Vec<AxisExpansion> {
    let mut out: Vec<AxisExpansion> = vec![AxisExpansion {
        paths: vec![Vec::with_capacity(template.len())],
        label: String::new(),
    }];
    for (pos, tmpl) in template.iter().enumerate() {
        match tmpl {
            SegmentTemplate::Key(k) => {
                let part = if pos == 0 { k.clone() } else { format!(".{k}") };
                for axis in &mut out {
                    axis.label.push_str(&part);
                    for path in &mut axis.paths {
                        path.push(Segment::Key(k.clone()));
                    }
                }
            }
            SegmentTemplate::Index(i) => {
                let part = format!("[{i}]");
                for axis in &mut out {
                    axis.label.push_str(&part);
                    for path in &mut axis.paths {
                        path.push(Segment::Index(*i));
                    }
                }
            }
            SegmentTemplate::Filter { key, value } => {
                let part = format!("[{}={}]", key, render_filter_value(value));
                for axis in &mut out {
                    axis.label.push_str(&part);
                    for path in &mut axis.paths {
                        path.push(Segment::Filter {
                            key: key.clone(),
                            value: value.clone(),
                        });
                    }
                }
            }
            SegmentTemplate::IndexRange {
                start,
                end,
                inclusive,
            } => {
                let last = if *inclusive { *end } else { *end - 1 };
                let mut new_out = Vec::new();
                for axis in &out {
                    let mut i = *start;
                    while i <= last {
                        let mut copy = axis.clone();
                        let part = format!("[{i}]");
                        copy.label.push_str(&part);
                        for path in &mut copy.paths {
                            path.push(Segment::Index(i as usize));
                        }
                        new_out.push(copy);
                        i += 1;
                    }
                }
                out = new_out;
            }
            SegmentTemplate::KeyGroup(keys) => {
                let part = if pos == 0 {
                    format!("{{{}}}", keys.join(","))
                } else {
                    format!(".{{{}}}", keys.join(","))
                };
                for axis in &mut out {
                    axis.label.push_str(&part);
                    let mut new_paths = Vec::with_capacity(axis.paths.len() * keys.len());
                    for path in &axis.paths {
                        for k in keys {
                            let mut copy = path.clone();
                            copy.push(Segment::Key(k.clone()));
                            new_paths.push(copy);
                        }
                    }
                    axis.paths = new_paths;
                }
            }
        }
    }
    out
}

fn render_filter_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
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

    // ---------- key groups (brace expansion) ----------

    #[test]
    fn parse_key_group_two_keys() {
        assert_eq!(
            parse("treasury.ideal.{food,wood}"),
            vec![
                SegmentTemplate::Key("treasury".into()),
                SegmentTemplate::Key("ideal".into()),
                SegmentTemplate::KeyGroup(vec!["food".into(), "wood".into()]),
            ]
        );
    }

    #[test]
    fn parse_key_group_single_key_is_allowed() {
        assert_eq!(
            parse("a.{x}"),
            vec![
                SegmentTemplate::Key("a".into()),
                SegmentTemplate::KeyGroup(vec!["x".into()]),
            ]
        );
    }

    #[test]
    fn parse_key_group_trims_whitespace() {
        assert_eq!(
            parse("a.{ x , y ,z }"),
            vec![
                SegmentTemplate::Key("a".into()),
                SegmentTemplate::KeyGroup(vec!["x".into(), "y".into(), "z".into()]),
            ]
        );
    }

    #[test]
    fn parse_key_group_followed_by_more_path() {
        assert_eq!(
            parse("a.{x,y}.z"),
            vec![
                SegmentTemplate::Key("a".into()),
                SegmentTemplate::KeyGroup(vec!["x".into(), "y".into()]),
                SegmentTemplate::Key("z".into()),
            ]
        );
    }

    #[test]
    fn rejects_empty_key_group() {
        err("a.{}");
    }

    #[test]
    fn rejects_key_group_with_blank_entry() {
        err("a.{x,,y}");
        err("a.{,x}");
        err("a.{x,}");
    }

    #[test]
    fn rejects_key_group_with_non_ident() {
        err("a.{1foo}");
        err("a.{x.y}");
    }

    #[test]
    fn rejects_unterminated_key_group() {
        err("a.{x,y");
    }

    // ---------- expand_template ----------

    #[test]
    fn expand_no_ranges_returns_one_axis_one_path() {
        let t = parse("a.b[5].c");
        let exp = expand_template(&t);
        assert_eq!(exp.len(), 1);
        assert_eq!(exp[0].paths.len(), 1);
        assert_eq!(
            exp[0].paths[0],
            vec![
                Segment::Key("a".into()),
                Segment::Key("b".into()),
                Segment::Index(5),
                Segment::Key("c".into()),
            ]
        );
        assert_eq!(exp[0].label, "a.b[5].c");
    }

    #[test]
    fn expand_single_range() {
        let t = parse("classes[0..=2].weight");
        let exp = expand_template(&t);
        assert_eq!(exp.len(), 3);
        let last_idx_for_each: Vec<usize> = exp
            .iter()
            .map(|ax| match ax.paths[0][1] {
                Segment::Index(i) => i,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(last_idx_for_each, vec![0, 1, 2]);
        assert_eq!(exp[0].label, "classes[0].weight");
        assert_eq!(exp[2].label, "classes[2].weight");
    }

    #[test]
    fn expand_two_ranges_cartesian() {
        let t = parse("a[0..=1].b[0..=1]");
        let exp = expand_template(&t);
        // 2 × 2 = 4 axes, leftmost varies slowest, each axis has 1 path.
        assert_eq!(exp.len(), 4);
        let idx_pairs: Vec<(usize, usize)> = exp
            .iter()
            .map(|ax| match (&ax.paths[0][1], &ax.paths[0][3]) {
                (Segment::Index(i), Segment::Index(j)) => (*i, *j),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(idx_pairs, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn expand_key_group_couples_paths_in_one_axis() {
        let t = parse("treasury.{food,wood,ore}");
        let exp = expand_template(&t);
        assert_eq!(exp.len(), 1, "key-group does not split axes");
        assert_eq!(exp[0].paths.len(), 3);
        assert_eq!(
            exp[0].paths[0],
            vec![Segment::Key("treasury".into()), Segment::Key("food".into())]
        );
        assert_eq!(
            exp[0].paths[2],
            vec![Segment::Key("treasury".into()), Segment::Key("ore".into())]
        );
        assert_eq!(exp[0].label, "treasury.{food,wood,ore}");
    }

    #[test]
    fn expand_range_and_key_group_combine() {
        let t = parse("a[0..=1].{x,y}");
        let exp = expand_template(&t);
        // 2 independent axes, each with 2 coupled paths.
        assert_eq!(exp.len(), 2);
        assert_eq!(exp[0].paths.len(), 2);
        assert_eq!(exp[1].paths.len(), 2);
        assert_eq!(exp[0].label, "a[0].{x,y}");
        assert_eq!(exp[1].label, "a[1].{x,y}");
    }

    #[test]
    fn expand_two_key_groups_take_cartesian_product_of_paths() {
        let t = parse("a.{x,y}.{p,q}");
        let exp = expand_template(&t);
        assert_eq!(exp.len(), 1);
        assert_eq!(exp[0].paths.len(), 4);
        let leaf_pairs: Vec<(String, String)> = exp[0]
            .paths
            .iter()
            .map(|p| match (&p[1], &p[2]) {
                (Segment::Key(a), Segment::Key(b)) => (a.clone(), b.clone()),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            leaf_pairs,
            vec![
                ("x".into(), "p".into()),
                ("x".into(), "q".into()),
                ("y".into(), "p".into()),
                ("y".into(), "q".into()),
            ]
        );
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
