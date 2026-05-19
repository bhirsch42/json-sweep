# Changelog

All notable changes to this project will be documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
