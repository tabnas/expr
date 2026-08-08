# ci/

Staging area for GitHub Actions workflow changes.

**Currently empty — nothing is pending.** The staged workflow that used to
live here (`build.yml`) has been promoted: `.github/workflows/ci.yml` is now
the org-standard thin caller that delegates to
`tabnas/.github/.github/workflows/polyglot-ci.yml@main`, and the old
`.github/workflows/build.yml` (which cloned and built the `@tabnas` siblings
by hand) is gone.

This directory exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file here.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

Note that most CI behaviour (matrix, Node version, `core.autocrlf false`)
now lives in the shared reusable workflow, not in this repo — change it
there rather than staging a local override.
