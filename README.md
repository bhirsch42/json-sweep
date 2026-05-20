# json_sweep

[![CI](https://github.com/bhirsch42/json-sweep/actions/workflows/ci.yml/badge.svg)](https://github.com/bhirsch42/json-sweep/actions/workflows/ci.yml)

Fan out a base JSON config into N variants by sweeping values along one
or more axes. The binary is `jswp`.

```bash
$ cat base.json
{"model": {"lr": 0.01}, "training": {"seed": 0}}

$ jswp base.json training.seed=1,2,3 model.lr=0.01,0.1
{"model":{"lr":0.01},"training":{"seed":1}}
{"model":{"lr":0.1},"training":{"seed":1}}
{"model":{"lr":0.01},"training":{"seed":2}}
{"model":{"lr":0.1},"training":{"seed":2}}
{"model":{"lr":0.01},"training":{"seed":3}}
{"model":{"lr":0.1},"training":{"seed":3}}
```

That's the whole tool: cartesian (or zipped) product of axis values,
deep-merged into a base, emitted as NDJSON. There's no sweep-file
format; the sweep *is* the argv, so you commit a shell command rather
than a YAML spec. For everything `jswp` doesn't do (single edits,
overlays, filtering, queries), reach for [`jq`](https://jqlang.github.io/jq/).
See [`USING_WITH_JQ.md`](USING_WITH_JQ.md) for compositions.

## Install

From a checkout:

```bash
cargo install --path .
```

This puts `jswp` on your `PATH`. (Not yet published to crates.io.)

## More examples

```bash
# 20 seeds, one file each, plus a manifest
jswp base.json training.seed=1..=20 --out-dir seeds/

# Pipe the base in
cat base.json | jswp training.seed=1..=10 model.lr=0.01,0.1,0.3

# Categorical variants
jswp base.json dataset=cifar10,imagenet,mnist

# Stepped float range
jswp base.json model.dropout=0..=1:0.25     # 0, 0.25, 0.5, 0.75, 1.0

# Zip three correlated axes (no cartesian)
jswp base.json --zip \
  optimizer=adam,sgd,adamw \
  model.lr=0.001,0.01,0.001 \
  model.momentum=0.0,0.9,0.0

# Structured-JSON values
jswp base.json \
  'augment={"flip":true},{"flip":true,"crop":32}'

# Sweep a value buried inside an array, addressed by sibling key
jswp portfolio.json \
  'spec.classes[name=Treasury].ideal.equity_target=0..=0.5:0.1'

# Sweep every weight in a known-length array independently
jswp portfolio.json 'weights[0..=5]=0.1,0.2,0.3'

# Pipe through jq upstream, GNU parallel downstream
jq -s '.[0] * .[1]' defaults.json experiment.json \
  | jswp training.seed=1..=5 optimizer=adam,sgd --out-dir runs/ \
  | parallel -j8 'my-runner --config {} > {}.out'
```

## Invocation

```
jswp [BASE] PATH=GEN [PATH=GEN...] [options]

  --zip                 zip mode (default: cartesian)
  --out-dir DIR         write files to DIR; print paths on stdout
  --with-axes           wrap stdout NDJSON as {axes, config}
  --pretty              indent emitted JSON
  --max N               refuse if cardinality > N (default 10000)
  --strict-paths        error if any axis path doesn't resolve in BASE
```

Positional args are split by whether they contain `=`:

- `PATH=GEN` (contains `=`) is an axis.
- Anything else is the BASE file path. At most one.
- If stdin is piped and no BASE positional is given, stdin is the base.
  To force stdin even with a BASE positional present, redirect from
  `/dev/null` to suppress the auto-fill.
- `-` is a valid BASE meaning stdin, for scripts that want to be
  explicit.

At least one axis is required.

## Path syntax (left of `=`)

Dot-separated path into nested JSON: `training.seed`,
`model.optimizer.momentum`. Keys containing literal `.` are out of scope;
preprocess with `jq` if you need them.

Array elements can be addressed three ways:

| Form                   | Meaning                                                                |
| ---------------------- | ---------------------------------------------------------------------- |
| `classes[5]`           | element at index 5 (0-based). Out-of-bounds is an error.               |
| `classes[name=Treasury]` | first element where `obj.name == "Treasury"`. Zero matches is an error. |
| `classes[0..=5]`       | range — the path itself expands to one axis per index (see below).     |

Brackets can chain (`a[0][1]`) and can appear at the start of a path
(`[3].name`) when the base is an array. Filter values can be numbers
(`[id=42]`), bare strings (`[name=Treasury]`, `[id=t-1]`), quoted JSON
strings (`[name="with space"]`), `true`/`false`, or `null`. The filter
matches by serde_json value equality on the chosen object key.

### Bracket ranges expand the path itself

A range inside brackets (`a..b` or `a..=b`) makes the path *generative*:
each index becomes its own independent axis, sharing the same GEN.
Multiple ranges in one path take the cartesian product.

```bash
jswp base.json 'xs[0..=2]=1,2'
# expands to 3 axes (xs[0], xs[1], xs[2]), each with values {1,2}
# → 2^3 = 8 variants
```

If you instead want one axis that writes the same value to multiple
indices, list them explicitly with separate axes that share GEN
generation, or preprocess with `jq`.

### Brace-expansion couples paths into one axis

A `.{k1,k2,...}` group writes the same swept value to each named key
under the parent. It produces **one** axis, not many — the opposite of
bracket-range semantics.

```bash
jswp base.json 'treasury.{food,wood,ore,metal,tools}=50,500,5000'
# one axis with 3 values → 3 variants
# in each variant, all five treasury.* keys share that value
```

This is the right tool when several JSON locations are logically the
same knob. Combine with filters and ranges as needed:

```bash
jswp base.json 'spec.classes[name=Treasury].ideal.{food,wood}=10,20'
jswp base.json 'xs[0..=1].{a,b}=1,2'  # 2 independent axes × 2 vals = 4 variants;
                                       # within each, a and b move together
```

Group entries must be plain identifiers (no indices, filters, or
nested brackets). Multiple groups in one path take the cartesian
product of paths within the same axis.

## Generator syntax (right of `=`)

Every form is typeable without shell quoting (except when a value
itself needs quoting, e.g., strings with spaces or JSON literals).

| Form              | Expands to                                                |
| ----------------- | --------------------------------------------------------- |
| `v1,v2,...,vn`    | the literal list `[v1, v2, …, vn]`                        |
| `a..b`            | integers `a, a+1, …, b-1` (half-open). Integer args only. |
| `a..=b`           | integers `a, a+1, …, b` (inclusive). Integer args only.   |
| `a..b:s`          | `a, a+s, a+2s, …` while `< b`. Floats allowed.            |
| `a..=b:s`         | `a, a+s, a+2s, …` while `≤ b`. Floats allowed.            |

Float stepped ranges accumulate as `a + i*s` rather than by repeated
addition, to avoid drift.

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
isn't `,`. This catches identifiers, kebab-case, and even relative
paths like `./variant.json`. To force JSON-string parsing, quote it.

## Modes

- **cartesian** (default): every combination of axis values.
- **`--zip`**: parallel iteration; all axes must produce the same
  number of values. Cardinality = that length.

### Derived values via `--zip`

`jswp` doesn't compute expressions (no `capacity = ideal * 10`).
For derived knobs, enumerate the paired values explicitly with
`--zip`:

```bash
jswp base.json --zip \
  'treasury.{food,wood,ore,metal,tools}=50,500,5000' \
  'treasury.capacity=500,5000,50000'
```

Three variants, one per column, with `capacity` co-varying with the
`ideal` block.

## Merge semantics

For each combination of axis values, `jswp` deep-merges the resulting
`{path → value}` map into the base:

- Objects merge recursively.
- Non-objects (numbers, strings, arrays, null) replace.
- Arrays always replace; they don't concatenate.
- Missing intermediate objects are created.

## Output

### Default: NDJSON to stdout

One compact JSON value per line, in iteration order. Axes vary in
left-to-right declaration order, with the **rightmost axis varying
fastest** (matching how nested `for` loops read).

```bash
jswp base.json a=1,2 b=10,20 > runs.ndjson
# emits: a=1,b=10  a=1,b=20  a=2,b=10  a=2,b=20
```

### `--out-dir DIR`

Write each config to a numbered file (`0001.json`, `0002.json`, …),
print the paths to stdout, and write `DIR/manifest.ndjson`:

```json
{"path": "0001.json", "axes": {"training.seed": 1, "model.lr": 0.01}}
{"path": "0002.json", "axes": {"training.seed": 1, "model.lr": 0.1}}
```

### `--with-axes`

Wrap each NDJSON line as `{"axes": {...}, "config": {...}}`. Useful
for correlating without writing files. Mutually exclusive with
`--out-dir`.

### `--pretty`

Indent emitted JSON. In NDJSON mode this means multi-line values
(not strict one-per-line), so chain through `jq -c` if a consumer
needs strict NDJSON.

### `--max N` (default 10000)

Refuse before generating if the product exceeds `N`.

### `--strict-paths`

Default behavior is permissive: a `Key` segment that doesn't exist in
the base is auto-created (see *Merge semantics*), which means a typo
silently produces variants identical to the base. Pass `--strict-paths`
to refuse before generating if any axis path doesn't resolve to an
existing slot in the base. Index, filter, and range segments are
already strict; this flag only tightens up `Key` segments.

## Exit codes

- `0` — success.
- `1` — user error: malformed `PATH=GEN`, sweep cardinality > `--max`,
  zip with mismatched axis lengths, no axes given, path traverses an
  incompatible value in the base (e.g., array index out of bounds, key
  on a non-object, filter on a non-array, filter with zero matches).
- `2` — input error: malformed JSON in the base, IO failure.

## Contributing

Bug reports and small PRs welcome at
<https://github.com/bhirsch42/json-sweep>. For larger changes, open
an issue first.

## License

MIT — see [LICENSE](LICENSE).
