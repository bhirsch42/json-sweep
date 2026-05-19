# Using `jswp` with `jq`

`jswp` fans a base config out across axis values. Everything else
(composing layered configs, single-value edits, querying paths,
post-processing results) is `jq`'s job. This document shows how they
fit together.

If you don't have `jq`: <https://jqlang.github.io/jq/>.

## Mental model

```
[ jq … ]     →   [ jswp … ]   →   [ jq … ]   →   [ runner / parallel / xargs ]
   ↑                ↑                 ↑
   compose          fan out           reshape
   (one config)     (N configs)       (per-config)
```

`jq` upstream prepares **one** base config. `jswp` turns it into **N**.
`jq` downstream reshapes each of those N (filter fields, extract
values, re-merge with per-run overlays).

## Upstream: building the base with `jq`

### Layer overlays before sweeping

```bash
jq -s '.[0] * .[1]' defaults.json experiment.json \
  | jswp training.seed=1..=5 optimizer=adam,sgd
```

`jq -s '.[0] * .[1]'` deep-merges two JSONs (slurp mode + multiplication).
`jswp` reads the merged base from stdin.

### Three layers, base → environment → experiment

```bash
jq -s '.[0] * .[1] * .[2]' defaults.json env.staging.json experiment.json \
  | jswp training.seed=1..=5
```

### Build the base inline

```bash
jq -n '{training: {seed: 0}, model: {lr: 0.01, optimizer: "adam"}}' \
  | jswp training.seed=1..=5
```

`jq -n` ("null input") lets you construct JSON from nothing. Handy for
quick experiments without a checked-in base file.

### Splice in a sub-tree from another file

```bash
jq --slurpfile m model_presets/large.json '.model = $m[0]' base.json \
  | jswp training.seed=1..=10
```

## Downstream: shaping `jswp` output with `jq`

`jswp` emits NDJSON (one JSON value per line). `jq` reads NDJSON
natively, no `-s` needed unless you want to collect into an array.

### Pretty-print the stream

```bash
jswp base.json training.seed=1..=3 | jq .
```

### Extract just the axis values per config

```bash
jswp base.json training.seed=1..=3 model.lr=0.01,0.1 --with-axes \
  | jq -c '.axes'
# {"training.seed":1,"model.lr":0.01}
# {"training.seed":1,"model.lr":0.1}
# …
```

### Collect into a single array

```bash
jswp base.json training.seed=1..=3 | jq -s .
```

### Filter to a subset of the sweep

```bash
jswp base.json training.seed=1..=20 --with-axes \
  | jq -c 'select(.axes."training.seed" % 2 == 0) | .config'
```

### Reshape an `--out-dir` manifest

```bash
jswp base.json training.seed=1..=10 --out-dir runs/
jq -s 'group_by(.axes."training.seed")
       | map({seed: .[0].axes."training.seed", paths: map(.path)})' \
  runs/manifest.ndjson
```

## Per-config edits with `jq` after `jswp`

When you need a tweak that doesn't fit the axis model (a derived value,
a conditional override):

```bash
jswp base.json training.seed=1..=5 \
  | jq -c '.model.warmup = (.model.lr * 100)'
```

Or branch on axis values when using `--with-axes`:

```bash
jswp base.json optimizer=adam,sgd --with-axes \
  | jq -c '
      .config.model.momentum =
        (if .axes.optimizer == "sgd" then 0.9 else 0 end)
      | .config
    '
```

## When to reach for `jq` instead of `jswp`

`jswp` is for cartesian or zipped fan-out across axes. For a single
edit or a single merge, `jq` alone is shorter:

```bash
# Single edit
jq '.training.seed = 42' base.json

# Merge two configs
jq -s '.[0] * .[1]' base.json overlay.json
```

`jswp` requires at least one axis, so it'll refuse a no-axis call and
point you here.

## Merging notes: `jswp` vs. `jq`

Both `jswp` and `jq`'s `*` operator deep-merge objects and replace
otherwise. Arrays replace; they do not concatenate. If you need array
concat, do the merge with `jq` first and pass the result to `jswp`:

```bash
jq -s '
  def deepmerge(a; b):
    if (a|type) == "object" and (b|type) == "object" then
      reduce ([a,b] | add | keys_unsorted[]) as $k ({};
        .[$k] = deepmerge(a[$k]; b[$k]))
    elif (a|type) == "array" and (b|type) == "array" then a + b
    else if b == null then a else b end end;
  deepmerge(.[0]; .[1])
' defaults.json overlay.json \
  | jswp training.seed=1..=5
```

## Shell helpers worth keeping around

If you reach for merges often, drop these into your shell rc:

```bash
# Deep merge any number of JSON files (jq `*` reduced across inputs).
jqm()  { jq -s 'reduce .[] as $x ({}; . * $x)' "$@"; }

# Shallow merge (top-level keys only; later files win key-by-key).
jqms() { jq -s 'reduce .[] as $x ({}; . + $x)' "$@"; }
```

Then `jqm defaults.json overlay.json | jswp …` reads cleanly.

## Strict NDJSON

`jswp --pretty` emits indented JSON, which is *not* strict NDJSON
(values span multiple lines). If a downstream consumer needs one JSON
value per line, pipe through `jq -c`:

```bash
jswp base.json training.seed=1..=5 --pretty | jq -c .
```

## Common pitfalls

- **`-r` strips quotes.** `jq -r` is for getting a string out of a JSON
  string. Don't `-r` something you intend to keep parsing as JSON.
- **`*` is deep merge; `+` is shallow.** Both with quirks: `*` recurses
  into objects only (arrays still replace); `+` does object merge at
  the top level only.
- **`select` returns nothing on miss**, not null. `jq -c 'select(...)'`
  yields fewer lines than the input. If you want the full stream with
  a per-line marker, use
  `jq -c '. as $c | {match: (… condition …), config: $c}'`.
- **`--with-axes` and `--out-dir` are mutually exclusive.** If you
  want both, use `--out-dir` and read `manifest.ndjson` afterwards.
