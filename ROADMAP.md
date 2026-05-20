# jswp Roadmap

This roadmap captures planned improvements to `jswp`, prioritized from real-world
usage feedback. The motivating use case and suggested order of attack are
preserved below.

## Status

| #   | Item                                  | Status                                |
| --- | ------------------------------------- | ------------------------------------- |
| 0   | `Cursor` helper in `path.rs`          | DONE                                  |
| 1   | Array indexing                        | DONE (commit bd51a68)                 |
| 2   | Filter-by-field                       | DONE (commit bd51a68)                 |
| 3   | Coupled paths from one axis           | DONE (brace expansion)                |
| 4   | Derived values                        | MOOTED by 3 + `--zip` (README example) |
| 5   | Validate axis paths resolve           | DONE (opt-in `--strict-paths`)        |
| P   | `--with-axes` embed / sidecar         | DONE (stdout wrap + `--out-dir` manifest) |
| P   | Manifest to stdout when no `--out-dir`| DONE-equivalent via `--with-axes`     |

## Motivating use case

Sweeping Treasury params in `backend/scenarios/money-source.json`. Treasury is
the 6th element of the `spec.classes` array. Desired axes:

- `Treasury.count` — single path inside the array element
- `Treasury.ideal.{food,wood,ore,metal,tools}` — five paths, all driven by one knob
- `Treasury.capacity` — derived from `ideal` (e.g., `ideal * 10`)
- `econ.seed` — the only one `jswp` handles natively today

The composed sweep was 3 × 3 × 3 = 27 variants. The user fell back to a bash
loop with a `jq … | with_entries(.value = $i)` preprocess per variant and
dropped `jswp` entirely.

## What works well today — don't break

- Range/list/zip axis syntax is clean: `econ.seed=1..=10`, `temp=0.5,1.0,2.0`,
  `--zip` for paired axes.
- The deep-merge layering with `jqm` upstream and `jswp` downstream is the right
  separation of concerns. The gap is **path expressiveness**, not the
  composition model.

## Priorities

### 0. Internal: `Cursor` helper in `path.rs`

Extract a small `Cursor<&str>` (peek / bump / eat_while / at_eof) to replace the
manual `self.pos += 1` + `bytes[i]` indexing throughout the path parser. Keep
the precise column-offset errors (`PathError::at`) and the existing
lookahead-friendly structure — the string-aware `find_close_bracket` /
`find_top_level_eq` scans and the `..` vs `..=` disambiguation don't change
shape, they just read more cleanly. Not a rewrite, and explicitly *not* a port
to a parser-combinator crate (`nom` was considered and rejected: it would
regress error quality and fits the lookahead bits awkwardly).

Worth doing before priorities 1–3 below, since each of those extends the path
grammar and will be easier on top of a tidier scanner.

### 1. Array indexing — blocker

Positional indexing into arrays:

```
spec.classes.5.ideal.food=50,500,5000
```

The current "out of scope" note in the README means anything inside
`classes[]`, `commodities[]`, or `recipes[]` falls outside `jswp` and has to be
preprocessed with `jq`. That covers ~all class-level params on real scenarios.
Whatever flat-path tweaks are addressable today (e.g., `econ.seed`,
`spec.population_cap`) are a small minority of the knobs scenarios actually
expose.

### 2. Filter-by-field — strong follow-up

Address array elements by a field value rather than position:

```
spec.classes[name=Treasury].ideal.food=…
```

(or a JSONPath-style `[?(@.name=='Treasury')]`). Robust to array reordering and
self-documenting at the call site. Without this, `classes.5` silently does the
wrong thing if a class is added or moved.

### 3. Coupled paths from one axis

A common pattern: one logical knob fans out to several JSON paths. In the
motivating case, all five `Treasury.ideal.<commodity>` entries share the same
swept value. Possible shapes:

- **Brace-expansion syntax**:
  `spec.classes[name=Treasury].ideal.{food,wood,ore,metal,tools}=50,500,5000`
- **Named alias / config block**: define `treasury_ideal` once with its target
  paths, then sweep `treasury_ideal=…` on the CLI.
- The existing structured-JSON-value form (`ideal='{"food":50,…}'`)
  technically works but is verbose enough that users reach for `jq` instead.

### 4. Derived values — lower priority

A path whose value is a function of another swept axis — e.g.,
`capacity = ideal * 10`. Most ambitious of these. If (3) ships, this is mostly
mootable: the user can switch to `--zip` mode and enumerate
`(ideal, capacity)` pairs explicitly.

### 5. Validate axis paths resolve

Loud error if an axis path doesn't address anything in the base config. The
implicit deep-merge today means a typo in a path silently produces variants
that are identical to the base — easy to miss until you stare at outputs.

## Polish (nice, not blocking)

- **`--with-axes`**: embed the axis assignment into each emitted variant
  (`{"_axes": {…}, …}` or a sidecar). The README already references this as
  upstream of richer summaries.
- **Manifest to stdout** when no `--out-dir` is given, so NDJSON streaming
  pipelines can still recover axis metadata.

## Suggested order of attack

1 → 2 → 3 covers the bulk of the ergonomics gap. 4 is optional polish once 3
lands. 5 is small and worth bundling with 1.
