# ci/

Scripts that a workflow runs and a contributor can run too, kept here so
that a hosted run and a local one cannot say different things. The
workflows themselves live in `.github/workflows/`.

- **`rust/run.sh`** is the Rust gate: formatting, build, tests,
  doctests, clippy, rustdoc with broken intra-doc links fatal, and the
  lockfile check, under the MSRV toolchain from `rs/Cargo.toml` when it
  is installed. `.github/workflows/rust.yml` clones the sibling crates
  the `rs/` crate takes by path (`parser`, `json`, `jsonic`, `support`,
  `debug`), installs that toolchain with rustup and runs the script. To
  run it locally, clone the same five repositories beside this one.

This directory used to be the staging area for workflow files, which a
maintainer then promoted into `.github/workflows/` (admin `DECISIONS.md`
ADR-8). Everything staged here has been promoted: `build.yml` was
replaced by the org-standard `ci.yml` caller, and `docs.yml` (the Vale
prose gate) and `rust.yml` (the Rust gate) now live in
`.github/workflows/`.

Most CI behaviour (matrix, Node version, `core.autocrlf false`) lives in
the shared reusable workflow that `ci.yml` calls,
`tabnas/.github/.github/workflows/polyglot-ci.yml`. Change it there
rather than adding a local override.
