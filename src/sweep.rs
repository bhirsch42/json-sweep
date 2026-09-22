use crate::merge::{ApplyError, apply_segments};
use crate::path::Segment;
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct Axis {
    /// One or more coupled paths sharing the swept value. A vanilla axis
    /// has a single path; a `.{a,b,c}` key-group axis has one path per
    /// brace entry.
    pub paths: Vec<Vec<Segment>>,
    /// Display label, e.g. `treasury.{food,wood,ore}` or `classes[5].x`.
    pub label: String,
    pub values: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Cross,
    Zip,
}

#[derive(Debug, Clone)]
pub struct SweepItem {
    pub axes: Map<String, Value>,
    pub config: Value,
}

#[derive(Debug)]
pub enum SweepError {
    TooLarge {
        cardinality: usize,
        max: usize,
    },
    ZipMismatch {
        axis_index: usize,
        axis_path: String,
        expected: usize,
        got: usize,
    },
    EmptyAxis {
        axis_path: String,
    },
    NoAxes,
    Apply {
        axis_path: String,
        inner: ApplyError,
    },
}

impl std::fmt::Display for SweepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SweepError::TooLarge { cardinality, max } => {
                write!(
                    f,
                    "sweep would produce {cardinality} configs, exceeds --max {max}"
                )
            }
            SweepError::ZipMismatch {
                axis_index,
                axis_path,
                expected,
                got,
            } => write!(
                f,
                "zip length mismatch: axis {axis_index} ({axis_path:?}) has {got} values, expected {expected}"
            ),
            SweepError::EmptyAxis { axis_path } => {
                write!(f, "axis {axis_path:?} produced zero values")
            }
            SweepError::NoAxes => write!(f, "at least one axis is required"),
            SweepError::Apply { axis_path, inner } => {
                write!(f, "applying axis {axis_path:?}: {inner}")
            }
        }
    }
}

impl std::error::Error for SweepError {}

pub fn cardinality(axes: &[Axis], mode: Mode) -> Result<usize, SweepError> {
    if axes.is_empty() {
        return Err(SweepError::NoAxes);
    }
    for a in axes {
        if a.values.is_empty() {
            return Err(SweepError::EmptyAxis {
                axis_path: a.label.clone(),
            });
        }
    }
    match mode {
        Mode::Cross => {
            let mut n: usize = 1;
            for a in axes {
                n = n.checked_mul(a.values.len()).ok_or(SweepError::TooLarge {
                    cardinality: usize::MAX,
                    max: usize::MAX,
                })?;
            }
            Ok(n)
        }
        Mode::Zip => {
            let expected = axes[0].values.len();
            for (i, a) in axes.iter().enumerate().skip(1) {
                if a.values.len() != expected {
                    return Err(SweepError::ZipMismatch {
                        axis_index: i,
                        axis_path: a.label.clone(),
                        expected,
                        got: a.values.len(),
                    });
                }
            }
            Ok(expected)
        }
    }
}

pub fn expand(
    base: &Value,
    axes: &[Axis],
    mode: Mode,
    max: usize,
) -> Result<Vec<SweepItem>, SweepError> {
    let n = cardinality(axes, mode)?;
    if n > max {
        return Err(SweepError::TooLarge {
            cardinality: n,
            max,
        });
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let indices = index_for(i, axes, mode);
        let mut config = base.clone();
        let mut axes_map = Map::new();
        for (axis, &idx) in axes.iter().zip(indices.iter()) {
            let value = &axis.values[idx];
            for path in &axis.paths {
                apply_segments(&mut config, path, value).map_err(|e| SweepError::Apply {
                    axis_path: axis.label.clone(),
                    inner: e,
                })?;
            }
            axes_map.insert(axis.label.clone(), value.clone());
        }
        out.push(SweepItem {
            axes: axes_map,
            config,
        });
    }
    Ok(out)
}

fn index_for(i: usize, axes: &[Axis], mode: Mode) -> Vec<usize> {
    match mode {
        Mode::Zip => vec![i; axes.len()],
        Mode::Cross => {
            let mut indices = vec![0usize; axes.len()];
            let mut stride: usize = 1;
            for k in (0..axes.len()).rev() {
                let len = axes[k].values.len();
                indices[k] = (i / stride) % len;
                stride *= len;
            }
            indices
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn axis(path: &str, values: Vec<Value>) -> Axis {
        let segs: Vec<Segment> = path
            .split('.')
            .map(|s| Segment::Key(s.to_string()))
            .collect();
        Axis {
            paths: vec![segs],
            label: path.to_string(),
            values,
        }
    }

    #[test]
    fn cardinality_cross_product() {
        let axes = vec![
            axis("a", vec![json!(1), json!(2), json!(3)]),
            axis("b", vec![json!(10), json!(20)]),
            axis("c", vec![json!(100), json!(200), json!(300), json!(400)]),
        ];
        assert_eq!(cardinality(&axes, Mode::Cross).unwrap(), 24);
    }

    #[test]
    fn cross_ordering_rightmost_fastest() {
        let axes = vec![
            axis("a", vec![json!(1), json!(2)]),
            axis("b", vec![json!(10), json!(20)]),
        ];
        let items = expand(&json!({}), &axes, Mode::Cross, 100).unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].config, json!({"a": 1, "b": 10}));
        assert_eq!(items[1].config, json!({"a": 1, "b": 20}));
        assert_eq!(items[2].config, json!({"a": 2, "b": 10}));
        assert_eq!(items[3].config, json!({"a": 2, "b": 20}));
    }

    #[test]
    fn zip_equal_lengths() {
        let axes = vec![
            axis("a", vec![json!(1), json!(2), json!(3)]),
            axis("b", vec![json!(10), json!(20), json!(30)]),
        ];
        let items = expand(&json!({}), &axes, Mode::Zip, 100).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].config, json!({"a": 1, "b": 10}));
        assert_eq!(items[1].config, json!({"a": 2, "b": 20}));
        assert_eq!(items[2].config, json!({"a": 3, "b": 30}));
    }

    #[test]
    fn zip_mismatched_lengths_error() {
        let axes = vec![
            axis("a", vec![json!(1), json!(2), json!(3)]),
            axis("b.c", vec![json!(10), json!(20)]),
        ];
        let err = expand(&json!({}), &axes, Mode::Zip, 100).unwrap_err();
        match err {
            SweepError::ZipMismatch {
                axis_index,
                axis_path,
                expected,
                got,
            } => {
                assert_eq!(axis_index, 1);
                assert_eq!(axis_path, "b.c");
                assert_eq!(expected, 3);
                assert_eq!(got, 2);
            }
            other => panic!("expected ZipMismatch, got {other:?}"),
        }
    }

    #[test]
    fn max_exceeded_errors_before_generating() {
        let axes = vec![
            axis("a", vec![json!(1); 10]),
            axis("b", vec![json!(2); 10]),
            axis("c", vec![json!(3); 10]),
        ];
        let err = expand(&json!({}), &axes, Mode::Cross, 100).unwrap_err();
        match err {
            SweepError::TooLarge { cardinality, max } => {
                assert_eq!(cardinality, 1000);
                assert_eq!(max, 100);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[test]
    fn axes_map_carries_source_values() {
        let axes = vec![
            axis("knobs.x", vec![json!(0.5), json!(1.0)]),
            axis("knobs.policy", vec![json!("argmax"), json!("softmax")]),
        ];
        let items = expand(&json!({}), &axes, Mode::Cross, 100).unwrap();
        assert_eq!(items[0].axes.get("knobs.x"), Some(&json!(0.5)));
        assert_eq!(items[0].axes.get("knobs.policy"), Some(&json!("argmax")));
        // Not stringified — values keep their JSON type.
        assert!(items[0].axes.get("knobs.x").unwrap().is_f64());
    }

    #[test]
    fn nested_path_applied_to_base() {
        let axes = vec![axis("knobs.spawn.temp", vec![json!(0.5), json!(1.0)])];
        let base = json!({"econ": {"seed": 1}});
        let items = expand(&base, &axes, Mode::Cross, 100).unwrap();
        assert_eq!(
            items[0].config,
            json!({"econ": {"seed": 1}, "knobs": {"spawn": {"temp": 0.5}}})
        );
    }

    #[test]
    fn coupled_paths_share_one_value() {
        // Simulate `treasury.{food,wood}=10,20`: one axis, two paths.
        let axis = Axis {
            paths: vec![
                vec![Segment::Key("treasury".into()), Segment::Key("food".into())],
                vec![Segment::Key("treasury".into()), Segment::Key("wood".into())],
            ],
            label: "treasury.{food,wood}".into(),
            values: vec![json!(10), json!(20)],
        };
        let items = expand(&json!({}), &[axis], Mode::Cross, 100).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].config,
            json!({"treasury": {"food": 10, "wood": 10}})
        );
        assert_eq!(
            items[1].config,
            json!({"treasury": {"food": 20, "wood": 20}})
        );
        assert_eq!(items[0].axes.get("treasury.{food,wood}"), Some(&json!(10)));
    }

    #[test]
    fn no_axes_errors() {
        let err = expand(&json!({}), &[], Mode::Cross, 100).unwrap_err();
        assert!(matches!(err, SweepError::NoAxes));
    }
}
