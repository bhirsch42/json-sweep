# Using `jswp` with `jq`

`jswp` deliberately does one thing: fan a base config out across axis
values. Everything else — composing layered configs, single-value edits,
querying paths, post-processing results — is `jq`'s job. This document
shows how they fit together.

If you don't have `jq`: <https://jqlang.github.io/jq/>.

## Mental model

```
[ jq … ]     →   [ jswp … ]   →   [ jq … ]   →   [ runner / parallel / xargs ]
   ↑                ↑                 ↑
   compose          fan out           reshape
   (one config)     (N configs)       (per-config)
```

`jq` upstream prepares **one** base config. `jswp` turns it into **N**.
`jq` downstream reshapes each of those N (filter fields, extract values,
re-merge with per-run overlays).

## Upstream: building the base with `jq`

### Layer overlays before sweeping

```bash
jq -s '.[0] * .[1]' defaults.json experiment.json \
  | jswp econ.seed=1..=5 policy=argmax,softmax
```

`jq -s '.[0] * .[1]'` deep-merges two JSONs (slurp mode + multiplication).
`jswp` reads the merged base from stdin.

### Three layers, base → environment → experiment

```bash
jq -s '.[0] * .[1] * .[2]' defaults.json env.staging.json experiment.json \
  | jswp econ.seed=1..=5
```

### Build the base inline

```bash
jq -n '{econ: {seed: 0}, knobs: {x: 1.0, policy: "argmax"}}' \
  | jswp econ.seed=1..=5
```

`jq -n` ("null input") lets you construct JSON from nothing — handy for
quick experiments without a checked-in base file.

### Splice in a sub-tree from another file

```bash
jq --slurpfile k knobs/aggressive.json '.knobs = $k[0]' base.json \
  | jswp econ.seed=1..=10
```

## Downstream: shaping `jswp` output with `jq`

`jswp` emits NDJSON (one JSON value per line). `jq` reads NDJSON natively
— no `-s` needed unless you want to collect into an array.

### Pretty-print the stream

```bash
jswp base.json econ.seed=1..=3 | jq .
```

### Extract just the axis values per config

```bash
jswp base.json econ.seed=1..=3 knobs.x=0.5,1.0 --with-axes \
  | jq -c '.axes'
# {"econ.seed":1,"knobs.x":0.5}
# {"econ.seed":1,"knobs.x":1.0}
# …
```

### Collect into a single array

```bash
jswp base.json econ.seed=1..=3 | jq -s .
```

### Filter to a subset of the sweep

```bash
jswp base.json econ.seed=1..=20 --with-axes \
  | jq -c 'select(.axes."econ.seed" % 2 == 0) | .config'
```

### Reshape an `--out-dir` manifest

```bash
jswp base.json econ.seed=1..=10 --out-dir runs/
jq -s 'group_by(.axes."econ.seed") | map({seed: .[0].axes."econ.seed", paths: map(.path)})' \
  runs/manifest.ndjson
```

## Per-config edits with `jq` after `jswp`

When you need a tweak that doesn't fit the axis model (e.g., a derived
value, a conditional override):

```bash
jswp base.json econ.seed=1..=5 \
  | jq -c '.knobs.derived = (.knobs.x * .econ.seed)'
```

Or branch on axis values when using `--with-axes`:

```bash
jswp base.json policy=argmax,softmax --with-axes \
  | jq -c '
      .config.knobs.temp = (if .axes.policy == "softmax" then 1.0 else 0 end)
      | .config
    '
```

## When to reach for `jq` instead of `jswp`

`jswp` is for **cartesian/zipped fan-out across axes**. If you just want
to set a value or merge two configs, `jq` alone is shorter:

```bash
# Single edit — no need for jswp
jq '.econ.seed = 42' base.json

# Merge two configs — no need for jswp
jq -s '.[0] * .[1]' base.json overlay.json
```

A sweep with no axes is just `jq` — `jswp` refuses it.

## Merging notes — `jswp` vs. `jq`

Both `jswp` and `jq`'s `*` operator do **deep merge of objects, replace
otherwise**. Arrays replace; they do not concatenate. If you need array
concat, build the merged base with `jq` first using a custom recipe (see
below) and pass the result to `jswp`.

```bash
# Concat-arrays merge in jq, then sweep
jq -s '
  def deepmerge(a; b):
    if (a|type) == "object" and (b|type) == "object" then
      reduce ([a,b] | add | keys_unsorted[]) as $k ({};
        .[$k] = deepmerge(a[$k]; b[$k]))
    elif (a|type) == "array" and (b|type) == "array" then a + b
    else if b == null then a else b end end;
  deepmerge(.[0]; .[1])
' defaults.json overlay.json \
  | jswp econ.seed=1..=5
```

## Shell helpers worth keeping around

If you use deep/shallow merges a lot, the companion sourcing script in
the emergent-economies repo (`scripts/envrc.sh`) defines:

- `jqm a.json b.json …` — deep-merge any number of JSON files (`jq` `*`
  reduced across slurped inputs).
- `jqms a.json b.json …` — **shallow** merge (top-level keys only;
  later files overwrite earlier ones key-by-key, no recursion).

They're tiny — the script also adds `jswp` to your `PATH`. See the
script for definitions.

## Strict NDJSON

`jswp --pretty` emits indented JSON, which is **not** strict NDJSON
(values span multiple lines). If a downstream consumer needs one JSON
value per line, pipe through `jq -c`:

```bash
jswp base.json econ.seed=1..=5 --pretty | jq -c .
```

## Common pitfalls

- **`-r` strips quotes.** `jq -r` is for getting a string out of a JSON
  string. Don't `-r` something you intend to keep parsing as JSON.
- **`*` is deep merge; `+` is shallow.** Both with quirks: `*` recurses
  into objects only (arrays still replace); `+` does object merge at the
  top level only.
- **`select` returns nothing on miss**, not null. `jq -c 'select(...)'`
  will yield fewer lines than the input — that's the design. If you want
  the full stream with a per-line marker, do `jq -c '. as $c | {match: (… condition …), config: $c}'`.
- **`jswp --with-axes` and `jswp --out-dir` are mutually exclusive.** If
  you want both per-config axes and on-disk files, use `--out-dir` and
  read `manifest.ndjson` afterwards.
