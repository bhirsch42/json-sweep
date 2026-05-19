use crate::sweep::SweepItem;
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

/// Read all of stdin if it's non-TTY. Returns None if stdin is a terminal or
/// if it's non-TTY but contains zero bytes (`< /dev/null` suppresses auto-fill).
pub fn buffered_stdin() -> io::Result<Option<Vec<u8>>> {
    if io::stdin().is_terminal() {
        return Ok(None);
    }
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf)?;
    if buf.is_empty() { Ok(None) } else { Ok(Some(buf)) }
}

pub fn read_base(source: BaseSource) -> io::Result<Value> {
    let mut text = String::new();
    match source {
        BaseSource::StdinBuffer(bytes) => {
            text = String::from_utf8(bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        }
        BaseSource::File(path) => {
            File::open(&path)?.read_to_string(&mut text)?;
        }
    }
    serde_json::from_str(&text).map_err(io_invalid_json)
}

fn io_invalid_json(e: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("invalid JSON: {e}"))
}

#[derive(Debug, Clone)]
pub enum BaseSource {
    StdinBuffer(Vec<u8>),
    File(PathBuf),
}

pub fn write_ndjson_stdout(
    items: &[SweepItem],
    pretty: bool,
    with_axes: bool,
) -> io::Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for item in items {
        let value = if with_axes {
            json!({ "axes": Value::Object(item.axes.clone()), "config": item.config })
        } else {
            item.config.clone()
        };
        let text = if pretty {
            serde_json::to_string_pretty(&value)
        } else {
            serde_json::to_string(&value)
        }
        .map_err(io_invalid_json)?;
        out.write_all(text.as_bytes())?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

pub fn write_out_dir(
    items: &[SweepItem],
    dir: &Path,
    pretty: bool,
) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let width = file_width(items.len());
    let manifest_path = dir.join("manifest.ndjson");
    let mut manifest = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&manifest_path)?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    for (i, item) in items.iter().enumerate() {
        let name = format!("{:0width$}.json", i + 1, width = width);
        let file_path = dir.join(&name);
        let text = if pretty {
            serde_json::to_string_pretty(&item.config)
        } else {
            serde_json::to_string(&item.config)
        }
        .map_err(io_invalid_json)?;
        std::fs::write(&file_path, format!("{text}\n"))?;
        writeln!(stdout, "{}", file_path.display())?;

        let manifest_line = json!({
            "path": name,
            "axes": Value::Object(item.axes.clone()),
        });
        let line = serde_json::to_string(&manifest_line).map_err(io_invalid_json)?;
        manifest.write_all(line.as_bytes())?;
        manifest.write_all(b"\n")?;
    }
    Ok(())
}

fn file_width(cardinality: usize) -> usize {
    let natural = cardinality.to_string().len();
    natural.max(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_clamps_to_four() {
        assert_eq!(file_width(1), 4);
        assert_eq!(file_width(9999), 4);
        assert_eq!(file_width(10_000), 5);
        assert_eq!(file_width(123_456), 6);
    }
}
