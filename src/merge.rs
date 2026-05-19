use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct ApplyError {
    pub path_segment: String,
    pub encountered: &'static str,
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "path traverses {} at segment {:?}",
            self.encountered, self.path_segment
        )
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

pub fn apply_axis(base: &mut Value, path: &[String], value: &Value) -> Result<(), ApplyError> {
    if path.is_empty() {
        merge_into(base, value);
        return Ok(());
    }
    if !base.is_object() {
        *base = Value::Object(Map::new());
    }
    let mut cur = base;
    for seg in &path[..path.len() - 1] {
        let obj = match cur {
            Value::Object(m) => m,
            other => {
                return Err(ApplyError {
                    path_segment: seg.clone(),
                    encountered: kind_name(other),
                });
            }
        };
        let next = obj
            .entry(seg.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if !next.is_object() {
            return Err(ApplyError {
                path_segment: seg.clone(),
                encountered: kind_name(next),
            });
        }
        cur = next;
    }
    let last = path.last().unwrap().clone();
    let obj = match cur {
        Value::Object(m) => m,
        other => {
            return Err(ApplyError {
                path_segment: last,
                encountered: kind_name(other),
            });
        }
    };
    match obj.get_mut(&last) {
        Some(slot) => merge_into(slot, value),
        None => {
            obj.insert(last, value.clone());
        }
    }
    Ok(())
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

    fn segs(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
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
    fn object_replaces_non_object() {
        let mut base = json!({"x": 5});
        merge_into(&mut base, &json!({"x": {"a": 1}}));
        assert_eq!(base, json!({"x": {"a": 1}}));
    }

    #[test]
    fn non_object_replaces_object() {
        let mut base = json!({"x": {"a": 1}});
        merge_into(&mut base, &json!({"x": 7}));
        assert_eq!(base, json!({"x": 7}));
    }

    #[test]
    fn apply_axis_creates_missing_intermediates() {
        let mut base = json!({});
        apply_axis(&mut base, &segs(&["a", "b", "c"]), &json!(42)).unwrap();
        assert_eq!(base, json!({"a": {"b": {"c": 42}}}));
    }

    #[test]
    fn apply_axis_preserves_siblings() {
        let mut base = json!({"a": {"b": 1, "z": 99}});
        apply_axis(&mut base, &segs(&["a", "c"]), &json!(7)).unwrap();
        assert_eq!(base, json!({"a": {"b": 1, "z": 99, "c": 7}}));
    }

    #[test]
    fn apply_axis_deep_merges_objects_at_leaf() {
        let mut base = json!({"a": {"x": 1, "y": 2}});
        apply_axis(&mut base, &segs(&["a"]), &json!({"y": 99, "z": 3})).unwrap();
        assert_eq!(base, json!({"a": {"x": 1, "y": 99, "z": 3}}));
    }

    #[test]
    fn apply_axis_replace_scalar_with_object() {
        let mut base = json!({"a": 5});
        apply_axis(&mut base, &segs(&["a"]), &json!({"k": 1})).unwrap();
        assert_eq!(base, json!({"a": {"k": 1}}));
    }

    #[test]
    fn apply_axis_through_non_object_errors() {
        let mut base = json!({"a": 5});
        let err = apply_axis(&mut base, &segs(&["a", "b"]), &json!(1)).unwrap_err();
        assert_eq!(err.path_segment, "a");
        assert_eq!(err.encountered, "number");
    }

    #[test]
    fn apply_axis_through_array_errors() {
        let mut base = json!({"a": [1, 2]});
        let err = apply_axis(&mut base, &segs(&["a", "b"]), &json!(1)).unwrap_err();
        assert_eq!(err.path_segment, "a");
        assert_eq!(err.encountered, "array");
    }
}
