# ADR 0024 — Release process, versioning, and cross-platform CI

- **Status:** Accepted
- **Date:** 2026-06-30

## Context

The project is approaching its first release. We want two things:

- A *release* — and only a release — to build Linux, Windows, and macOS binaries
  with the version stamped in, published as a tagged GitHub Release with the
  binaries attached.
- Routine commits to keep the version number correct (major/minor/patch) *without*
  building anything, so the next release's number is always ready.

Two forces complicate the second point. First, a version means different things
for the two crates. `rvision` is a library: a version is a *contract* with
downstream code ("major" = "I broke your build"). `edit` is a binary: a version
is a changelog signal, since nothing links against it. Second, the framework is
meant to outlive the editor — there are plans to use `rvision` from other
applications — so it will eventually want its own independent version line and
probably its own repository.

Today, though, `edit` depends on `rvision` through a `path` dependency, which
ignores the version field entirely. An independent `rvision` version *right now*
would be a contract with an audience that does not yet exist. And the expensive
part of an eventual split — moving `rvision` to its own repo and switching `edit`
from a path-dep to a published-dep — costs the same whether the two versioned in
lockstep or independently up to that point.

The project's ethos is hand-rolling with a tight *runtime* crate budget (ADR
0001/0006/0013). That budget governs Rust dependencies, not CI; neither GitHub
Actions nor a release bot touches it. But the spirit still argues for hand-rolling
anything that teaches us something and is cheap to own.

## Decision

**Versioning — Conventional Commits + release-please, single workspace version.**

- Both crates inherit one version from `[workspace.package]` via
  `version.workspace = true`; there is a single source of truth and a single tag
  (`vMAJOR.MINOR.PATCH`). `edit` and `rvision` move in lockstep. The first release
  bootstraps at `v0.1.0`.
- Commit messages follow **Conventional Commits**, *scoped per crate* from day one
  — `feat(rvision): …`, `fix(edit): …`, `feat(rvision)!: …`. Even while the
  versions are locked together, the scoped history carries the per-crate signal
  needed to fork `rvision`'s changelog out cleanly at the split.
- **release-please** (a GitHub Action) runs on every push to `main`. It reads the
  conventional commits and maintains an open "release PR" that bumps the workspace
  version and updates the changelog. This **does not build**; the pending version
  simply lives in that PR until we choose to cut a release.

We adopt release-please rather than hand-roll the semver brain: deriving a bump
from commit history is fiddly bookkeeping with real edge cases, and getting it
wrong teaches us nothing about Rust. It rides on CI, outside the runtime budget.

**Release — merging the release PR cuts the tag; the tag triggers the build.**

- Merging release-please's PR creates the git tag and the GitHub Release.
- The build is a hand-rolled, *gated downstream job in the same workflow*, fired by
  release-please's `release_created` output — not a separate workflow keyed on the
  tag. A tag or release created with the default `GITHUB_TOKEN` does **not** trigger
  another workflow run (GitHub's recursion guard), so a same-run gated job is the
  reliable wiring and needs no personal access token. It runs a matrix covering
  Linux (`x86_64-unknown-linux-gnu`), Windows
  (`x86_64-pc-windows-msvc`), and macOS on **both** Apple Silicon
  (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`) — the Intel target
  matters because the only physical Mac on hand is Intel — builds `edit --release`
  for each, and attaches the binaries to the Release. We hand-roll this: it is
  short, educational YAML, and only the cross-platform-build half, not the semver
  half.
- Version stamping is essentially free: the binary already reads
  `env!("CARGO_PKG_VERSION")` (the About box does this today), so it is correct as
  long as the tag and `Cargo.toml` agree — which release-please guarantees. A
  small `build.rs` may additionally bake the short git SHA so About can read
  `edit 0.1.0 (abc1234)`.
- The build is an `edit` concern only: a library has no binary to ship.
  `rvision`'s sole future "release" act is publishing to crates.io.

**The documented exit — when lockstep graduates to independence.**

The trigger to split: `rvision` gains a second consumer, or we decide to publish
it to crates.io. At that point we (a) move `rvision` to its own repository, (b)
start its real independent semver contract (pre-1.0 history under lockstep is
discardable — `0.x` semver licenses "anything may change"), and (c) switch `edit`
to depend on a published/tagged `rvision` rather than a path. Until then the code
stays decoupled by the standing rule that `rvision` carries no editor knowledge
(ADR 0003/0012) — *that*, not the shared version number, is the real seam.

## Consequences

- Daily process is simple: one version, one tag, one changelog, one release PR.
  Cutting a release is "merge the PR."
- The version number is always current and reviewable (it lives in an open PR)
  with no build cost — exactly the "set the number without building" goal.
- Builds happen only when intended (tag/release), keeping CI minutes down and
  routine pushes fast.
- We take on one third-party GitHub Action (release-please) on the release path.
  Mitigation: it touches no runtime code, and the build workflow — the part most
  worth owning — is ours.
- Scoped Conventional Commits become a standing discipline (`feat(rvision):` /
  `fix(edit):`). Cheap, and the single thing that makes the eventual split low-cost.
- Preparatory edits land with the implementation: `version.workspace = true` in
  both crates, a real `version` in `[workspace.package]`, and filling the empty
  `repository` field.
- The macOS/Windows build doubles as the "verify on Windows and macOS" task Phase
  10 already lists.

## Alternatives considered

- **Independent versions from the start** (release-please monorepo mode, per-crate
  tags/changelogs). Matches the eventual end-state, but pays a real tax now — two
  release PRs, two tags, commit-scope routing — to serve a downstream `rvision`
  audience that does not exist while the dependency is a path-dep. The split's cost
  is unchanged by paying it early, so this is over-investment pre-1.0.
- **Hand-rolled semver script.** Fully ours and in the project's spirit, but it
  reinvents release-please's commit-parsing and version-edge-case handling without
  teaching any Rust — the wrong thing to hand-roll. (We *do* hand-roll the build.)
- **cargo-dist / dist.** Purpose-built for cross-platform Rust release artefacts
  and installers. More capable than we need, and it owns the very build workflow we
  want to write ourselves for the learning value. Revisit if the hand-rolled matrix
  grows painful (universal macOS binaries, installers, checksums/signing).
- **Manual version bumps + manual releases.** Zero tooling, but the version drifts
  from reality and the "always-ready number" goal is lost; error-prone at exactly
  the moment — a release — when you least want mistakes.
