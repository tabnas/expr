# ci/

Staging area for GitHub Actions workflow changes.

The staged workflow that used to live here (`build.yml`) has been promoted:
`.github/workflows/ci.yml` is now the org-standard thin caller that delegates
to `tabnas/.github/.github/workflows/polyglot-ci.yml@main`, and the old
`.github/workflows/build.yml` (which cloned and built the `@tabnas` siblings
by hand) is gone.

Pending here:

- **`workflows/docs.yml`**, the prose gate: Vale over the reader-facing
  pages at the levels `.vale.ini` sets.
- **`workflows/rust.yml`**, the Rust gate: it clones the sibling crates the
  `rs/` crate takes by path (`parser`, `json`, `jsonic`, `support`), installs
  the MSRV toolchain from `rs/Cargo.toml`, and runs `ci/rust/run.sh`
  (formatting, build, tests, doctests, clippy, rustdoc with broken
  intra-doc links fatal, and the lockfile check). The
  script is the whole gate, so a local run and a hosted one cannot say
  different things.

This directory exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file here.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

Note that most CI behaviour (matrix, Node version, `core.autocrlf false`)
now lives in the shared reusable workflow, not in this repo — change it
there rather than staging a local override.
