# Changelog

All notable changes to this project will be documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Path syntax supports array element addressing:
  - `classes[5]` — numeric index.
  - `classes[name=Treasury]` — match the first array element by a
    sibling key (numbers, bare/quoted strings, bools, and null are
    valid filter values).
  - `classes[0..=5]` — range expands the path itself into N independent
    axes sharing one GEN (cartesian if multiple ranges appear).
- Path heads may now be bracket forms (`[3].name`) for array-rooted
  bases.

### Changed

- `ApplyError` is now an enum (`TraverseNonObject`, `TraverseNonArray`,
  `IndexOutOfBounds`, `FilterOnNonArray`, `FilterNoMatch`) for clearer
  failure messages.
- The internal `apply_axis` helper is renamed to `apply_segments` and
  takes `&[Segment]`.

### Removed

- `nom` and `anyhow` dependencies; the path parser is now hand-rolled
  to produce offset-precise error messages, and nothing else used
  either crate.

## [0.1.0] - 2026-05-19

Initial release.

### Added

- `jswp` CLI: expands a base JSON config across one or more axes,
  emitting the cartesian or zipped product as NDJSON.
- `PATH=GEN` axis syntax with list, integer-range, and stepped-range
  generators; bare strings, JSON literals, numbers, booleans, and
  null as values.
- `--zip` for parallel iteration across axes.
- `--out-dir DIR` to write numbered files plus `manifest.ndjson`.
- `--with-axes` to wrap each output as `{axes, config}`.
- `--pretty` for indented output.
- `--max N` cardinality guard (default 10000).
- Stdin auto-fill for the base config when piped.

[Unreleased]: https://github.com/bhirsch42/json-sweep/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/bhirsch42/json-sweep/releases/tag/v0.1.0
