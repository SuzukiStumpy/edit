# Releasing

How a release of `edit` is cut. The *why* behind this setup is ADR 0024; this is
the operating manual.

## The model in one paragraph

Versioning is automated from [Conventional Commits](https://www.conventionalcommits.org).
On every push to `main`, [release-please](https://github.com/googleapis/release-please)
reads the commits since the last release and keeps a single open **release PR**
that bumps the version and updates `CHANGELOG.md` — it builds nothing. Merging that
PR tags `vX.Y.Z`, cuts the GitHub Release, and triggers the cross-platform build
that attaches the binaries. `rvision` moved to its own repository (and its own
release process, once it has one) after `edit` v1.0.0; this covers `edit` alone.

## Day to day

Write Conventional Commits and the version takes care of itself:

| Commit subject | Effect on the next version |
|----------------|----------------------------|
| `fix(edit): …` | patch (`0.1.0` → `0.1.1`) |
| `feat(edit): …` | minor (`0.1.0` → `0.2.0`) |
| `feat(edit)!: …` or a `BREAKING CHANGE:` footer | minor while pre-1.0, major thereafter |
| `docs:`, `test:`, `chore:`, `refactor:`, … | no changelog entry, no bump |

Pre-1.0, breaking changes are capped at a minor bump (`bump-minor-pre-major` in
`release-please-config.json`), so the project won't lurch to `1.0.0` on its first
breaking change. A non-Conventional subject is simply ignored for versioning —
release-please logs a parse warning and moves on.

## Cutting a release

1. Push your work to `main` as usual.
2. release-please opens (or updates) a PR titled **`chore(main): release X.Y.Z`**.
   Review the proposed version and changelog.
3. **Merge that PR.** That is the whole release action. It:
   - tags `vX.Y.Z` and creates the GitHub Release with the changelog as its notes;
   - runs the build matrix — Linux (`x86_64`), Windows (`x86_64`), and macOS on both
     Apple Silicon (`aarch64`) and Intel (`x86_64`) — and attaches one binary per
     target as a Release asset, named `edit-vX.Y.Z-<target>`.

Routine pushes between releases only refresh the open PR; they never build.

## What ends up in the binary

The version is compiled in from `Cargo.toml` via `CARGO_PKG_VERSION`, and
`crates/edit/build.rs` stamps the short commit hash into `EDIT_GIT_SHA`, so
**Help ▸ About** reads `edit X.Y.Z (sha)`. In a build with no git metadata the hash
is omitted and About shows just `edit X.Y.Z`.

## Where it all lives

| File | Role |
|------|------|
| `.github/workflows/release.yml` | release-please job + the gated cross-platform build |
| `.github/workflows/ci.yml` | test on all three OSes + fmt/clippy, on every push and PR |
| `release-please-config.json` | release-please settings (release type, bump rules, updaters) |
| `.release-please-manifest.json` | the current released version (release-please owns this) |
| `crates/edit/build.rs` | stamps the git SHA |
| `Cargo.toml` → `[workspace.package].version` | the single source of truth, bumped by release-please |

The build is a **gated job inside `release.yml`**, not a separate workflow keyed on
the tag: a tag or release created with the default `GITHUB_TOKEN` does not trigger
another workflow run (GitHub's recursion guard), so the build runs in the same run,
conditioned on release-please's `release_created` output. See ADR 0024.

## One-time repository setup

Already done for this repo; recorded for anyone forking it or reproducing the setup
elsewhere.

- **Allow Actions to open the release PR.** Settings → Actions → General → Workflow
  permissions → enable *"Allow GitHub Actions to create and approve pull requests"*
  (and click **Save** — it has its own button). Without it the release-please job
  fails with *"GitHub Actions is not permitted to create or approve pull requests."*
- **Bootstrapping the first version.** The first release was pinned to `v0.1.0` with
  a `Release-As: 0.1.0` footer on the commit that introduced this automation, rather
  than letting the bump be computed from the (then non-Conventional) history.

## Troubleshooting

- *release-please job fails on PR creation* → the repository permission above.
- *A CI check on the release PR shows "action_required"* → GitHub gates workflow runs
  on bot-created PR branches; approve it in the Actions tab or ignore it (it does not
  block merging unless you add branch protection requiring it).
- *The build job didn't run after a release* → confirm the `build` job's
  `if: needs.release-please.outputs.release_created == 'true'` actually saw a release;
  it only runs when the merged PR was the release PR.
