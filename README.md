# json_sweep

Expand a sweep of axis assignments into N concrete JSON configs.

Binary: **`jswp`**. One job: take a base JSON config and a set of axis
assignments on the command line, emit the cartesian (or zipped) product
of axis values deep-merged into the base. Schema-agnostic — `jswp` knows
nothing about the structure of the configs it touches.

There is no sweep-file format. The "spec" is the argv. To capture a sweep
for reproducibility, commit the shell command (or a shell script that
runs it). Editing single values, layering overlays, querying paths — all
left to [`jq`](https://jqlang.github.io/jq/). See
[`USING_WITH_JQ.md`](USING_WITH_JQ.md) for composition recipes.

## Install

```bash
cargo install --path .
# or, once published:
cargo install json_sweep
```

This puts `jswp` on your `PATH`.

## At a glance

```bash
# 3 × 3 = 9 merged configs to stdout (NDJSON, one per line)
jswp base.json econ.seed=1,2,3 knobs.softmax_temp=0.5,1.0,2.0

# 20 seeds, one file each, plus a manifest
jswp base.json econ.seed=1..=20 --out-dir seeds/

# Pipe in the base
cat base.json | jswp econ.seed=1..=10 knobs.x=0.5,1.0,2.0
```

## Invocation

```
jswp [BASE] PATH=GEN [PATH=GEN...] [options]

  options:
    --zip                 zip mode (default: cross)
    --out-dir DIR         write files to DIR; print paths on stdout
    --with-axes           wrap stdout NDJSON as {axes, config}
    --pretty              indent emitted JSON
    --max N               refuse if cardinality > N (default 10000)
```

Positional disambiguation:

- A positional **containing `=`** is parsed as `PATH=GEN` (an axis).
- A positional **without `=`** is the BASE file path. At most one.
- If stdin is piped (non-TTY) and no BASE positional is given, stdin is
  the base. To force stdin even with a BASE positional present, redirect
  from `/dev/null` to suppress the auto-fill.
- `-` is a valid BASE meaning "stdin," for scripts that want to be
  explicit.

At least one axis is required. A sweep with no axes is just `jq` — use
`jq` directly.

## Path syntax (left of `=`)

Dot-separated path into nested JSON objects: `econ.seed`,
`knobs.spawn.softmax_temp`. Object keys only. Array indexing and keys
containing literal `.` are out of scope; preprocess with `jq` if needed.

## Generator syntax (right of `=`)

All forms are typeable without shell quoting (except when a value itself
needs quoting — e.g., strings with spaces or JSON literals). Each
generator expands to a sequence of JSON values.

| Form              | Expands to                                                |
| ----------------- | --------------------------------------------------------- |
| `v1,v2,...,vn`    | the literal list `[v1, v2, …, vn]`                        |
| `a..b`            | integers `a, a+1, …, b-1` (half-open). Integer args only. |
| `a..=b`           | integers `a, a+1, …, b` (inclusive). Integer args only.   |
| `a..b:s`          | `a, a+s, a+2s, …` while `< b`. Floats allowed.            |
| `a..=b:s`         | `a, a+s, a+2s, …` while `≤ b`. Floats allowed.            |

Float stepped ranges accumulate as `a + i*s`, not by repeated addition,
to avoid drift.

### Values inside a list

| Token                        | Parses as                                  |
| ---------------------------- | ------------------------------------------ |
| `42`, `3.14`, `-1.5e-3`      | JSON number                                |
| `true`, `false`              | JSON boolean                               |
| `null`                       | JSON null                                  |
| `argmax`, `kebab-case-thing` | bare string (any unquoted token not matching the above and not containing `,`) |
| `"with spaces"`              | quoted JSON string (needs shell escaping)  |
| `{"a":1}`, `[1,2,3]`         | JSON literal (needs shell escaping; commas inside are balanced via `{}`/`[]` nesting) |

A bare string starts with `[A-Za-z_/]` and continues with any char that
isn't `,`. This catches identifiers, kebab-case, and even relative paths
like `./variant.json`. To force JSON-string parsing, quote it.

### What's deliberately *not* supported

`linspace`, `logspace`, and other function-call forms aren't supported,
because parens require shell quoting and the whole point is that this CLI
doesn't. To get N evenly-spaced floats, type the list, use `a..=b:s` with
an appropriate step, or shell out: `$(python -c '...')`.

## Modes

- **Default** — cartesian product across all axes.
- **`--zip`** — parallel; all axis generators must produce the same
  number of values. Cardinality = that length.

## Merge semantics

For each combination of axis values, `jswp` deep-merges the resulting
`{path → value}` map into the base:

- Recursive merge for objects.
- Non-objects (numbers, strings, arrays, null) replace.
- Arrays always replace.
- Missing intermediate objects are created.

## Output

### Default: NDJSON to stdout

One compact JSON value per line, in iteration order — axes in
left-to-right declaration order, **rightmost axis varies fastest** under
cross mode (matches how nested `for` loops read).

```bash
jswp base.json a=1,2 b=10,20 > runs.ndjson
# emits: a=1,b=10  a=1,b=20  a=2,b=10  a=2,b=20
```

### `--out-dir DIR`

Write each config to a numbered file in `DIR` (`0001.json`,
`0002.json`, …), print the paths one per line on stdout, and write
`DIR/manifest.ndjson`:

```json
{"path": "0001.json", "axes": {"econ.seed": 1, "knobs.x": 0.5}}
{"path": "0002.json", "axes": {"econ.seed": 1, "knobs.x": 1.0}}
```

### `--with-axes`

Wrap each NDJSON line as `{"axes": {...}, "config": {...}}`. Useful for
correlating without writing files. Mutually exclusive with `--out-dir`.

### `--pretty`

Indent emitted JSON. In NDJSON mode this means multi-line JSON values —
not strict one-per-line — so chain through `jq -c` if a consumer needs
strict NDJSON.

### `--max N` (default 10000)

Refuse before generating if the product exceeds `N`. Prevents accidental
million-config blowouts.

## Examples

### Two-axis cross product

```bash
jswp base.json econ.seed=1,2,3 knobs.softmax_temp=0.5,1.0,2.0
```

### Single-axis seed sweep, one file per seed

```bash
jswp base.json econ.seed=1..=20 --out-dir seeds/
```

### Categorical variants

```bash
jswp base.json policy=directional,unconditional,off
```

### Stepped float range

```bash
jswp base.json knobs.x=0..=1:0.25      # 0, 0.25, 0.5, 0.75, 1.0
```

### Zip three correlated axes

```bash
jswp base.json --zip \
  policy=argmax,softmax,boltzmann \
  temp=0.0,1.0,1.5 \
  epsilon=0.0,0.0,0.1
```

### Structured-JSON sub-object presets

```bash
jswp base.json \
  'spawn={"policy":"argmax"},{"policy":"softmax","temp":1.0}'
```

### Compose with `jq` upstream, dispatch with `parallel` downstream

```bash
jq -s '.[0] * .[1]' defaults.json experiment.json \
  | jswp econ.seed=1..=5 policy=argmax,softmax --out-dir runs/ \
  | parallel -j8 'my-runner --config {} > {}.out'
```

See [`USING_WITH_JQ.md`](USING_WITH_JQ.md) for more.

## Exit codes

- `0` — success.
- `1` — user error: malformed `PATH=GEN`, sweep cardinality > `--max`,
  zip with mismatched axis lengths, no axes given, path traverses a
  non-object in the base.
- `2` — input error: malformed JSON in the base, IO failure.

## License

MIT — see [LICENSE](LICENSE).
