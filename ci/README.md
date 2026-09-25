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

Staging is no longer required. To change CI, edit `.github/workflows/`
in a reviewed pull request: session credentials can push workflow
changes (ADR-8, as amended on 2026-09-24). Sessions still cannot push
tags. Releases therefore go through `workflow_dispatch`, and a workflow
that runs only on a tag push needs a maintainer to push that tag.

Six of this repository's workflows also have a template in admin
`rollout/workflows/`: `ci.yml`, `crates-release.yml`, `deps-gate.yml`,
`notify-status.yml`, `release.yml` and `scorecard.yml`. ADR-8 as amended
says a workflow changed here is mirrored in its template, so change the
template too, in admin. Otherwise admin `scripts/verify.sh` reports the
drift, and the next `rollout/apply-workflows.sh --apply` pushes the old
text back. `clib.yml` and `clib-release.yml` are stamped (each carries a
`tabnas-clib-template` marker): they change only through admin
`tasks/clib-template/` and a re-stamp with `tasks/adopt-clib.sh`, never
by hand. The others have no template and change here alone.

Most CI behaviour (matrix, Node version, `core.autocrlf false`) lives in
the shared reusable workflow that `ci.yml` calls,
`tabnas/.github/.github/workflows/polyglot-ci.yml`. Change it there
rather than adding a local override.
