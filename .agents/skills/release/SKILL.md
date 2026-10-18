---
name: release
description: Make and publish a GitHub release of the current workspace version.
disable-model-invocation: true
---

Release the version in `[workspace.package]` in the root `Cargo.toml`, covering everything from the last release to `HEAD`. Run the commands in bash (Git Bash on Windows).

1. Find the CI run that built the release code: the latest green `CI` run on `main`. Commits that change only docs skip CI, so that run's commit can be older than `HEAD`.
2. Write `docs/release-notes/vX.Y.Z.md` in the style of the previous notes, covering every user-visible change since the last release's tag. Under "Before you upgrade", say whether remote machines need the new version too: they do exactly when `PROTOCOL` changed. Commit it as `Add release notes for X.Y.Z` and push.
3. Tag that commit `git tag -a vX.Y.Z -m "Adeline X.Y.Z"` and push the tag.
4. Download each artifact of the run as is into a new temp folder: `gh api repos/{owner}/{repo}/actions/artifacts/<id>/zip > <folder>/<artifact name>`. Each artifact is already the release zip, so keep it whole and keep its name. `gh run download` unpacks the zips, so use `gh api` instead.
5. Run `gh release create vX.Y.Z --verify-tag --title "Adeline X.Y.Z - <Name> Edition" --notes-file docs/release-notes/vX.Y.Z.md <folder>/*.zip`. Use the name the user gives, or ask for one.
6. Check that the release lists every artifact at its artifact size, then delete the temp folder.
