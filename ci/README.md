# ci/

Staging area for the updated GitHub Actions workflow.

[`build.yml`](build.yml) is the intended replacement for
[`.github/workflows/build.yml`](../.github/workflows/build.yml). It drops
the "clone and build the `@tabnas` siblings" steps, because those packages
are now published (all at `0.2.0`) and `expr/ts` resolves them from the npm
registry via a plain `npm i`. The job reduces to:

```yaml
    - name: Install, build and test
      shell: bash
      working-directory: expr/ts
      run: |
        set -e
        npm i
        npm run build
        npm test
```

It lives here (rather than directly under `.github/workflows/`) because
updating a workflow file requires the `workflow` OAuth scope, which the
automation that produced this change does not have. To apply it:

```bash
mv ci/build.yml .github/workflows/build.yml
rmdir ci 2>/dev/null || true   # if nothing else remains here
git commit -am "ci: resolve @tabnas deps from npm (drop sibling clone/build)"
```
