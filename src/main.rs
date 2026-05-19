use clap::Parser;
use json_sweep::generator::{GenError, parse_generator};
use json_sweep::io::{BaseSource, buffered_stdin, read_base, write_ndjson_stdout, write_out_dir};
use json_sweep::path::parse_path;
use json_sweep::sweep::{Axis, Mode, SweepError, expand};
use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "jswp",
    about = "Expand a sweep of axis assignments into N concrete JSON configs.",
    long_about = "\
jswp [BASE] PATH=GEN [PATH=GEN...] [options]

A positional containing `=` is an axis assignment.
A positional without `=` is the BASE file path (at most one). `-` means stdin.

Stdin auto-fill: if stdin is piped (non-TTY) and no BASE positional is given,
stdin is used as BASE. Gotcha: if you pipe AND pass a BASE positional, jswp
exits with an error — redirect from /dev/null to suppress the auto-fill."
)]
struct Args {
    /// Positional args: BASE (file path or `-`) and/or PATH=GEN axes.
    #[arg(value_name = "ARG")]
    positionals: Vec<String>,

    /// Zip mode (default: cross product).
    #[arg(long)]
    zip: bool,

    /// Write configs to DIR/0001.json…; print paths on stdout.
    #[arg(long, value_name = "DIR", conflicts_with = "with_axes")]
    out_dir: Option<PathBuf>,

    /// Wrap stdout NDJSON as {axes, config}.
    #[arg(long)]
    with_axes: bool,

    /// Pretty-print emitted JSON.
    #[arg(long)]
    pretty: bool,

    /// Refuse to expand if cardinality > N.
    #[arg(long, value_name = "N", default_value_t = 10_000)]
    max: usize,
}

#[derive(Debug)]
enum AppError {
    Usage(String),
    Input(String),
    Io(String),
}

impl AppError {
    fn exit_code(&self) -> u8 {
        match self {
            AppError::Usage(_) => 1,
            AppError::Input(_) => 2,
            AppError::Io(_) => 2,
        }
    }
    fn message(&self) -> &str {
        match self {
            AppError::Usage(m) | AppError::Input(m) | AppError::Io(m) => m,
        }
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("jswp: {}", e.message());
            ExitCode::from(e.exit_code())
        }
    }
}

fn run(args: Args) -> Result<(), AppError> {
    let stdin_buf = buffered_stdin().map_err(|e| AppError::Io(format!("reading stdin: {e}")))?;
    let (base_source, axis_args) = partition_positionals(&args.positionals, stdin_buf)?;
    if axis_args.is_empty() {
        return Err(AppError::Usage(
            "no axes given — pass at least one PATH=GEN positional".to_string(),
        ));
    }

    let axes = parse_axes(&axis_args)?;
    let base = load_base(base_source)?;
    let mode = if args.zip { Mode::Zip } else { Mode::Cross };

    let items = expand(&base, &axes, mode, args.max).map_err(sweep_error_to_app)?;

    if let Some(dir) = &args.out_dir {
        write_out_dir(&items, dir, args.pretty)
            .map_err(|e| AppError::Io(format!("writing --out-dir: {e}")))?;
    } else {
        write_ndjson_stdout(&items, args.pretty, args.with_axes)
            .map_err(|e| AppError::Io(format!("writing stdout: {e}")))?;
    }
    Ok(())
}

type AxisList<'a> = Vec<(usize, &'a str)>;

fn partition_positionals(
    positionals: &[String],
    stdin_buf: Option<Vec<u8>>,
) -> Result<(BaseSource, AxisList<'_>), AppError> {
    let mut base: Option<&str> = None;
    let mut axes: AxisList = Vec::new();
    for (i, raw) in positionals.iter().enumerate() {
        if raw == "-" || !raw.contains('=') {
            if let Some(existing) = base {
                return Err(AppError::Usage(format!(
                    "at most one BASE positional allowed; saw {existing:?} and {raw:?}"
                )));
            }
            base = Some(raw);
        } else {
            axes.push((i, raw.as_str()));
        }
    }

    let base_src = match (base, stdin_buf) {
        (Some("-"), Some(buf)) => BaseSource::StdinBuffer(buf),
        (Some("-"), None) => {
            return Err(AppError::Usage(
                "`-` requested stdin as BASE, but stdin is empty or a TTY".to_string(),
            ));
        }
        (Some(_), Some(_)) => {
            return Err(AppError::Usage(
                "stdin is piped AND a BASE positional was given; stdin has no slot, use `-` to position it (or `< /dev/null` to suppress)".to_string(),
            ));
        }
        (Some(path), None) => BaseSource::File(PathBuf::from(path)),
        (None, Some(buf)) => BaseSource::StdinBuffer(buf),
        (None, None) => {
            return Err(AppError::Usage(
                "no BASE provided: pass a JSON file path, `-` for stdin, or pipe into jswp"
                    .to_string(),
            ));
        }
    };
    Ok((base_src, axes))
}

fn load_base(src: BaseSource) -> Result<Value, AppError> {
    read_base(src).map_err(|e: std::io::Error| match e.kind() {
        std::io::ErrorKind::InvalidData => AppError::Input(format!("base JSON: {e}")),
        _ => AppError::Io(format!("reading base: {e}")),
    })
}

fn parse_axes(axis_args: &[(usize, &str)]) -> Result<Vec<Axis>, AppError> {
    let mut out = Vec::with_capacity(axis_args.len());
    for (idx, (_slot, raw)) in axis_args.iter().enumerate() {
        let axis_num = idx + 1;
        let eq = raw.find('=').expect("partition guarantees =");
        let path_str = &raw[..eq];
        let gen_str = &raw[eq + 1..];

        let (_, segs) = parse_path(path_str).map_err(|_| {
            AppError::Usage(format!(
                "axis {axis_num} ({raw:?}), col 1: invalid PATH {path_str:?}"
            ))
        })?;
        let generator = parse_generator(gen_str).map_err(|e: GenError| {
            let abs_col = path_str.len() + 2 + e.offset;
            AppError::Usage(format!(
                "axis {axis_num} ({raw:?}), col {abs_col}: {}",
                e.message
            ))
        })?;
        let values = generator.expand();
        out.push(Axis {
            path: segs,
            path_str: path_str.to_string(),
            values,
        });
    }
    Ok(out)
}

fn sweep_error_to_app(e: SweepError) -> AppError {
    match e {
        SweepError::TooLarge { cardinality, max } => AppError::Usage(format!(
            "sweep would produce {cardinality} configs, exceeds --max {max}"
        )),
        SweepError::ZipMismatch {
            axis_index,
            axis_path,
            expected,
            got,
        } => AppError::Usage(format!(
            "--zip length mismatch: axis {axis_path:?} (#{}) has {got} values, expected {expected}",
            axis_index + 1
        )),
        SweepError::EmptyAxis { axis_path } => {
            AppError::Usage(format!("axis {axis_path:?} produced zero values"))
        }
        SweepError::NoAxes => AppError::Usage("no axes given".to_string()),
        SweepError::Apply { axis_path, inner } => {
            AppError::Usage(format!("axis {axis_path:?}: {inner}"))
        }
    }
}
