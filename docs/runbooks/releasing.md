# Releasing Cooper

Versions follow [Semantic Versioning](https://semver.org) and come from the workspace
version in the root `Cargo.toml`. Commits follow
[Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `feat!:`
or a `BREAKING CHANGE:` footer), which drive the changelog. Decisions:
[ADR-0023](../adr/0023-deployment-observability-and-release-pipeline.md).

## Choosing the version

- **Major:** a breaking API change, a migration that needs manual steps, or a removed
  configuration variable. Before 1.0, use the minor number for these.
- **Minor:** new features (`feat:`).
- **Patch:** fixes only (`fix:`, `perf:`, security updates).

## Steps

1. On an up-to-date `main`, create a branch `release/vX.Y.Z`.
2. Set `version = "X.Y.Z"` under `[workspace.package]` in `Cargo.toml` and run
   `cargo check` so `Cargo.lock` updates.
3. Update `CHANGELOG.md` with [git-cliff](https://git-cliff.org):
   `git cliff --tag vX.Y.Z -o CHANGELOG.md`. Edit wording if needed.
4. Open a PR titled `chore(release): vX.Y.Z` and merge it after CI passes.
5. Tag the merge commit and push the tag:

   ```bash
   git switch main && git pull
   git tag -s vX.Y.Z -m "Cooper vX.Y.Z"
   git push origin vX.Y.Z
   ```

The **Release** workflow then:

- checks that the tag matches `Cargo.toml`;
- builds `ghcr.io/subanov/cooper` and `ghcr.io/subanov/cooper-migrate` for `linux/amd64`
  and `linux/arm64`, tagged `X.Y.Z` and `X.Y`, with SBOM and provenance attestations;
- signs both images with cosign (keyless, GitHub OIDC);
- creates the GitHub release with notes from git-cliff and CycloneDX SBOMs of
  `cooper-server` and `cooper-worker`.

A tag with a suffix (`v0.3.0-rc.1`) is published as a pre-release.

## Verifying an image

```bash
cosign verify ghcr.io/subanov/cooper:X.Y.Z \
  --certificate-identity-regexp 'https://github.com/subanov/cooper/.github/workflows/release.yml@refs/tags/v.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
docker buildx imagetools inspect ghcr.io/subanov/cooper:X.Y.Z --format '{{ json .SBOM }}'
```

## If the workflow fails

Fix the cause on `main` and release a new patch version. Do not move or reuse a tag that
has been pushed: images and signatures may already exist for it.
