use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;

fn write_base(dir: &TempDir, body: &Value) -> std::path::PathBuf {
    let p = dir.path().join("base.json");
    fs::write(&p, serde_json::to_string(body).unwrap()).unwrap();
    p
}

fn jswp() -> Command {
    Command::cargo_bin("jswp").unwrap()
}

#[test]
fn help_runs_and_prints_usage() {
    jswp()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn two_axis_cross_to_stdout() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({"econ": {"seed": 0}}));
    let out = jswp()
        .arg(&base)
        .arg("econ.seed=1,2")
        .arg("knobs.x=10,20")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "expected 4 NDJSON lines, got {text:?}");
    let vals: Vec<Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    // rightmost (knobs.x) varies fastest
    assert_eq!(vals[0], json!({"econ": {"seed": 1}, "knobs": {"x": 10}}));
    assert_eq!(vals[1], json!({"econ": {"seed": 1}, "knobs": {"x": 20}}));
    assert_eq!(vals[2], json!({"econ": {"seed": 2}, "knobs": {"x": 10}}));
    assert_eq!(vals[3], json!({"econ": {"seed": 2}, "knobs": {"x": 20}}));
}

#[test]
fn stdin_auto_fill() {
    let base = json!({"econ": {"seed": 0}});
    let out = jswp()
        .arg("econ.seed=1,2")
        .write_stdin(serde_json::to_string(&base).unwrap())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    let v: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v, json!({"econ": {"seed": 1}}));
}

#[test]
fn stdin_and_base_positional_conflict() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({"econ": {"seed": 0}}));
    jswp()
        .arg(&base)
        .arg("econ.seed=1,2")
        .write_stdin("{}")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("stdin"));
}

#[test]
fn out_dir_writes_numbered_files_and_manifest() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({"econ": {"seed": 0}}));
    let out_dir = dir.path().join("runs");
    jswp()
        .arg(&base)
        .arg("econ.seed=1,2")
        .arg("knobs.x=10,20")
        .arg("--out-dir")
        .arg(&out_dir)
        .assert()
        .success();
    for i in 1..=4 {
        let p = out_dir.join(format!("{:04}.json", i));
        assert!(p.exists(), "missing {}", p.display());
        let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert!(v.get("econ").is_some());
    }
    let manifest = fs::read_to_string(out_dir.join("manifest.ndjson")).unwrap();
    let lines: Vec<&str> = manifest.lines().collect();
    assert_eq!(lines.len(), 4);
    let first: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["path"], json!("0001.json"));
    assert_eq!(first["axes"]["econ.seed"], json!(1));
    assert_eq!(first["axes"]["knobs.x"], json!(10));
}

#[test]
fn with_axes_wraps_envelope() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({"econ": {"seed": 0}}));
    let out = jswp()
        .arg(&base)
        .arg("econ.seed=1,2")
        .arg("--with-axes")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    let v: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["axes"]["econ.seed"], json!(1));
    assert_eq!(v["config"]["econ"]["seed"], json!(1));
}

#[test]
fn with_axes_and_out_dir_conflict() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({}));
    jswp()
        .arg(&base)
        .arg("a=1,2")
        .arg("--with-axes")
        .arg("--out-dir")
        .arg(dir.path().join("runs"))
        .assert()
        .failure();
}

#[test]
fn zip_mismatched_lengths() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({}));
    jswp()
        .arg(&base)
        .arg("--zip")
        .arg("a=1,2,3")
        .arg("b=10,20")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"b\""));
}

#[test]
fn max_exceeded() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({}));
    jswp()
        .arg(&base)
        .arg("a=1..=10")
        .arg("b=1..=10")
        .arg("--max")
        .arg("50")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("100"))
        .stderr(predicate::str::contains("50"));
}

#[test]
fn malformed_base_json() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("base.json");
    fs::write(&base, "{not json").unwrap();
    jswp()
        .arg(&base)
        .arg("a=1,2")
        .assert()
        .code(2);
}

#[test]
fn malformed_generator() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({}));
    jswp()
        .arg(&base)
        .arg("a=")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("axis"));
}

#[test]
fn dash_means_stdin() {
    let base = json!({"x": 0});
    jswp()
        .arg("-")
        .arg("x=1,2")
        .write_stdin(serde_json::to_string(&base).unwrap())
        .assert()
        .success();
}

#[test]
fn no_axes_errors() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({}));
    jswp()
        .arg(&base)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no axes"));
}

#[test]
fn gen_error_includes_axis_number_and_column() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({}));
    jswp()
        .arg(&base)
        .arg("knobs.x=ok,too") // axis 1, valid
        .arg("econ.seed=foo..5") // axis 2, broken range
        .assert()
        .code(1)
        .stderr(predicate::str::contains("axis 2"))
        .stderr(predicate::str::contains("econ.seed=foo..5"))
        .stderr(predicate::str::contains("col 11"))
        .stderr(predicate::str::contains("expected integer"));
}

#[test]
fn bad_path_error_includes_axis_number() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({}));
    jswp()
        .arg(&base)
        .arg("1bad=1,2")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("axis 1"))
        .stderr(predicate::str::contains("invalid PATH"));
}

#[test]
fn path_traverses_non_object_error() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({"a": 5}));
    jswp()
        .arg(&base)
        .arg("a.b=1,2")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"a\""))
        .stderr(predicate::str::contains("number"));
}

#[test]
fn two_base_positionals_error() {
    let dir = TempDir::new().unwrap();
    let b1 = write_base(&dir, &json!({}));
    let b2 = dir.path().join("b2.json");
    fs::write(&b2, "{}").unwrap();
    jswp()
        .arg(&b1)
        .arg(&b2)
        .arg("a=1,2")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("BASE"));
}

#[test]
fn stepped_range_smoke() {
    let dir = TempDir::new().unwrap();
    let base = write_base(&dir, &json!({}));
    let out = jswp()
        .arg(&base)
        .arg("knobs.x=0..=1:0.25")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5);
}
