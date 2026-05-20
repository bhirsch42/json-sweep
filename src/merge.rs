use crate::path::Segment;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum ApplyError {
    /// Key segment hit a non-object slot (number, string, array, etc.).
    TraverseNonObject {
        segment: String,
        encountered: &'static str,
    },
    /// Index segment hit a slot that wasn't an array.
    TraverseNonArray {
        segment: String,
        encountered: &'static str,
    },
    /// Index segment was past the end of the array.
    IndexOutOfBounds {
        segment: String,
        index: usize,
        len: usize,
    },
    /// Filter segment hit a slot that wasn't an array.
    FilterOnNonArray {
        segment: String,
        encountered: &'static str,
    },
    /// Filter found zero matching array elements.
    FilterNoMatch { segment: String },
    /// Key segment didn't already exist in the parent object. Only surfaces
    /// from `check_segments`; `apply_segments` auto-creates instead.
    KeyMissing { segment: String },
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::TraverseNonObject {
                segment,
                encountered,
            } => write!(
                f,
                "path traverses {encountered} at segment {segment:?} (expected object)"
            ),
            ApplyError::TraverseNonArray {
                segment,
                encountered,
            } => write!(
                f,
                "path traverses {encountered} at segment {segment:?} (expected array)"
            ),
            ApplyError::IndexOutOfBounds {
                segment,
                index,
                len,
            } => write!(
                f,
                "index {index} out of bounds at segment {segment:?} (array length {len})"
            ),
            ApplyError::FilterOnNonArray {
                segment,
                encountered,
            } => write!(
                f,
                "filter {segment:?} applied to {encountered} (expected array)"
            ),
            ApplyError::FilterNoMatch { segment } => {
                write!(f, "filter {segment:?} matched zero elements")
            }
            ApplyError::KeyMissing { segment } => {
                write!(f, "key {segment:?} not present in base (typo?)")
            }
        }
    }
}

impl std::error::Error for ApplyError {}

pub fn merge_into(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(slot) => merge_into(slot, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, other) => {
            *slot = other.clone();
        }
    }
}

/// Walk `segs` through `base`, creating missing object intermediates for
/// `Key` segments, and deep-merge `value` into the leaf slot. `Index` and
/// `Filter` segments do not auto-create — they error if the slot isn't an
/// array of the right shape.
pub fn apply_segments(
    base: &mut Value,
    segs: &[Segment],
    value: &Value,
) -> Result<(), ApplyError> {
    if segs.is_empty() {
        merge_into(base, value);
        return Ok(());
    }
    // The first segment dictates the required base shape.
    match (&segs[0], &*base) {
        (Segment::Key(_), v) if !v.is_object() => {
            *base = Value::Object(Map::new());
        }
        _ => {}
    }

    let last_idx = segs.len() - 1;
    let mut cur: &mut Value = base;
    for (i, seg) in segs.iter().enumerate() {
        let is_leaf = i == last_idx;
        cur = step(cur, seg, is_leaf)?;
    }
    merge_into(cur, value);
    Ok(())
}

/// Verify that `segs` addresses an existing slot in `base`. Like
/// `apply_segments` but with zero auto-creation: a key that isn't already
/// present in the parent object returns `KeyMissing`. Used by the
/// `--strict-paths` validator to catch typo'd axis paths up front.
pub fn check_segments(base: &Value, segs: &[Segment]) -> Result<(), ApplyError> {
    let mut cur = base;
    for seg in segs {
        cur = match seg {
            Segment::Key(k) => {
                let obj = match cur {
                    Value::Object(m) => m,
                    other => {
                        return Err(ApplyError::TraverseNonObject {
                            segment: k.clone(),
                            encountered: kind_name(other),
                        });
                    }
                };
                obj.get(k)
                    .ok_or_else(|| ApplyError::KeyMissing { segment: k.clone() })?
            }
            Segment::Index(idx) => {
                let arr = match cur {
                    Value::Array(a) => a,
                    other => {
                        return Err(ApplyError::TraverseNonArray {
                            segment: format!("[{idx}]"),
                            encountered: kind_name(other),
                        });
                    }
                };
                if *idx >= arr.len() {
                    return Err(ApplyError::IndexOutOfBounds {
                        segment: format!("[{idx}]"),
                        index: *idx,
                        len: arr.len(),
                    });
                }
                &arr[*idx]
            }
            Segment::Filter { key, value: needle } => {
                let label = render_filter(key, needle);
                let arr = match cur {
                    Value::Array(a) => a,
                    other => {
                        return Err(ApplyError::FilterOnNonArray {
                            segment: label,
                            encountered: kind_name(other),
                        });
                    }
                };
                let pos = arr.iter().position(|elem| {
                    elem.as_object()
                        .and_then(|m| m.get(key))
                        .map(|v| v == needle)
                        .unwrap_or(false)
                });
                match pos {
                    Some(i) => &arr[i],
                    None => return Err(ApplyError::FilterNoMatch { segment: label }),
                }
            }
        };
    }
    Ok(())
}

fn step<'a>(
    cur: &'a mut Value,
    seg: &Segment,
    _is_leaf: bool,
) -> Result<&'a mut Value, ApplyError> {
    match seg {
        Segment::Key(k) => {
            let obj = match cur {
                Value::Object(m) => m,
                other => {
                    return Err(ApplyError::TraverseNonObject {
                        segment: k.clone(),
                        encountered: kind_name(other),
                    });
                }
            };
            // Auto-create missing object intermediates. If the next segment
            // needs a different shape (array for Index/Filter), it'll surface
            // a TraverseNonArray/FilterOnNonArray error against the existing
            // value the user put there.
            let entry = obj
                .entry(k.clone())
                .or_insert_with(|| Value::Object(Map::new()));
            Ok(entry)
        }
        Segment::Index(idx) => {
            let arr = match cur {
                Value::Array(a) => a,
                other => {
                    return Err(ApplyError::TraverseNonArray {
                        segment: format!("[{idx}]"),
                        encountered: kind_name(other),
                    });
                }
            };
            if *idx >= arr.len() {
                return Err(ApplyError::IndexOutOfBounds {
                    segment: format!("[{idx}]"),
                    index: *idx,
                    len: arr.len(),
                });
            }
            Ok(&mut arr[*idx])
        }
        Segment::Filter { key, value: needle } => {
            let label = render_filter(key, needle);
            let arr = match cur {
                Value::Array(a) => a,
                other => {
                    return Err(ApplyError::FilterOnNonArray {
                        segment: label,
                        encountered: kind_name(other),
                    });
                }
            };
            let pos = arr.iter().position(|elem| {
                elem.as_object()
                    .and_then(|m| m.get(key))
                    .map(|v| v == needle)
                    .unwrap_or(false)
            });
            match pos {
                Some(i) => Ok(&mut arr[i]),
                None => Err(ApplyError::FilterNoMatch { segment: label }),
            }
        }
    }
}

fn render_filter(key: &str, value: &Value) -> String {
    let v_str = match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    format!("[{key}={v_str}]")
}

fn kind_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(k: &str) -> Segment {
        Segment::Key(k.into())
    }
    fn idx(i: usize) -> Segment {
        Segment::Index(i)
    }
    fn filt(k: &str, v: Value) -> Segment {
        Segment::Filter {
            key: k.into(),
            value: v,
        }
    }

    #[test]
    fn scalar_overwrites_scalar() {
        let mut base = json!(1);
        merge_into(&mut base, &json!(2));
        assert_eq!(base, json!(2));
    }

    #[test]
    fn object_merges_recursively() {
        let mut base = json!({"a": 1, "b": {"x": 1, "y": 2}});
        merge_into(&mut base, &json!({"b": {"y": 99, "z": 3}, "c": 7}));
        assert_eq!(
            base,
            json!({"a": 1, "b": {"x": 1, "y": 99, "z": 3}, "c": 7})
        );
    }

    #[test]
    fn array_replaces_does_not_concat() {
        let mut base = json!({"xs": [1, 2, 3]});
        merge_into(&mut base, &json!({"xs": [9]}));
        assert_eq!(base, json!({"xs": [9]}));
    }

    #[test]
    fn apply_key_path_creates_missing_intermediates() {
        let mut base = json!({});
        apply_segments(&mut base, &[key("a"), key("b"), key("c")], &json!(42)).unwrap();
        assert_eq!(base, json!({"a": {"b": {"c": 42}}}));
    }

    #[test]
    fn apply_through_existing_array_index() {
        let mut base = json!({"classes": [{"weight": 0.1}, {"weight": 0.2}]});
        apply_segments(
            &mut base,
            &[key("classes"), idx(1), key("weight")],
            &json!(0.9),
        )
        .unwrap();
        assert_eq!(base["classes"][1]["weight"], json!(0.9));
        assert_eq!(base["classes"][0]["weight"], json!(0.1));
    }

    #[test]
    fn apply_to_array_index_with_object_deep_merges() {
        let mut base = json!({"classes": [{"weight": 0.1, "name": "A"}]});
        apply_segments(
            &mut base,
            &[key("classes"), idx(0)],
            &json!({"weight": 0.5, "extra": true}),
        )
        .unwrap();
        assert_eq!(
            base["classes"][0],
            json!({"weight": 0.5, "name": "A", "extra": true})
        );
    }

    #[test]
    fn index_out_of_bounds_errors() {
        let mut base = json!({"xs": [1, 2]});
        let err = apply_segments(&mut base, &[key("xs"), idx(5)], &json!(0)).unwrap_err();
        assert!(matches!(
            err,
            ApplyError::IndexOutOfBounds {
                index: 5,
                len: 2,
                ..
            }
        ));
    }

    #[test]
    fn index_on_non_array_errors() {
        let mut base = json!({"a": {"b": 1}});
        let err = apply_segments(&mut base, &[key("a"), idx(0)], &json!(0)).unwrap_err();
        match err {
            ApplyError::TraverseNonArray { encountered, .. } => assert_eq!(encountered, "object"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn filter_matches_first_array_element_by_key() {
        let mut base = json!({
            "classes": [
                {"name": "A", "weight": 0.1},
                {"name": "Treasury", "ideal": {"equity": 0.5}}
            ]
        });
        apply_segments(
            &mut base,
            &[
                key("classes"),
                filt("name", json!("Treasury")),
                key("ideal"),
                key("equity"),
            ],
            &json!(0.9),
        )
        .unwrap();
        assert_eq!(base["classes"][1]["ideal"]["equity"], json!(0.9));
        // Sibling preserved.
        assert_eq!(base["classes"][0]["weight"], json!(0.1));
    }

    #[test]
    fn filter_no_match_errors() {
        let mut base = json!({"classes": [{"name": "A"}]});
        let err = apply_segments(
            &mut base,
            &[key("classes"), filt("name", json!("Missing")), key("x")],
            &json!(1),
        )
        .unwrap_err();
        assert!(matches!(err, ApplyError::FilterNoMatch { .. }));
    }

    #[test]
    fn filter_on_non_array_errors() {
        let mut base = json!({"classes": {"a": 1}});
        let err = apply_segments(
            &mut base,
            &[key("classes"), filt("name", json!("A"))],
            &json!(1),
        )
        .unwrap_err();
        assert!(matches!(err, ApplyError::FilterOnNonArray { .. }));
    }

    #[test]
    fn check_segments_ok_when_path_resolves() {
        let base = json!({"econ": {"seed": 0}, "knobs": {"x": 1}});
        check_segments(&base, &[key("econ"), key("seed")]).unwrap();
        check_segments(&base, &[key("knobs"), key("x")]).unwrap();
    }

    #[test]
    fn check_segments_errors_on_missing_top_key() {
        let base = json!({"econ": {"seed": 0}});
        let err = check_segments(&base, &[key("knobs"), key("x")]).unwrap_err();
        match err {
            ApplyError::KeyMissing { segment } => assert_eq!(segment, "knobs"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn check_segments_errors_on_missing_leaf_key() {
        let base = json!({"econ": {"seed": 0}});
        let err = check_segments(&base, &[key("econ"), key("see")]).unwrap_err();
        match err {
            ApplyError::KeyMissing { segment } => assert_eq!(segment, "see"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn check_segments_resolves_through_index_and_filter() {
        let base = json!({"classes": [{"name": "A", "w": 1}, {"name": "T", "w": 2}]});
        check_segments(&base, &[key("classes"), idx(1), key("w")]).unwrap();
        check_segments(
            &base,
            &[key("classes"), filt("name", json!("T")), key("w")],
        )
        .unwrap();
    }

    #[test]
    fn check_segments_propagates_index_and_filter_errors() {
        let base = json!({"xs": [1, 2]});
        let err = check_segments(&base, &[key("xs"), idx(5)]).unwrap_err();
        assert!(matches!(err, ApplyError::IndexOutOfBounds { .. }));

        let base = json!({"xs": [{"name": "A"}]});
        let err = check_segments(&base, &[key("xs"), filt("name", json!("Z"))]).unwrap_err();
        assert!(matches!(err, ApplyError::FilterNoMatch { .. }));
    }

    #[test]
    fn key_traverse_non_object_errors() {
        let mut base = json!({"a": 5});
        let err = apply_segments(&mut base, &[key("a"), key("b")], &json!(1)).unwrap_err();
        match err {
            ApplyError::TraverseNonObject { encountered, .. } => assert_eq!(encountered, "number"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
