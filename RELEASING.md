# Releasing

All published crates share one version (`[workspace.package]` in the root
`Cargo.toml`), bumped and published with
[cargo-workspaces](https://github.com/pksunkara/cargo-workspaces).

**Published:** `interstice-abi`, `interstice-abi-macros`, `interstice-core`,
`interstice-client`, `interstice-sdk`, `interstice-sdk-core`,
`interstice-sdk-macros`, `interstice-cli`.

**Not published:** `interstice-ui`, `interstice-bench`, and every module under
`modules/`.

## Steps

1. **CI is green** on `main`, including the browser job.
2. **The modules are built** with the SDK being released:
   `scripts/build_modules.sh`. Built modules are not in git (DESIGN §10):
   `cargo publish` packages the default modules from
   `crates/interstice-cli/module_defaults/`, and the release workflow builds
   the examples itself and attaches them to the GitHub release, where
   `interstice example` downloads them.
3. **The changelog is ready.** Rename "Unreleased" in `CHANGELOG.md` to the
   new version.
4. **Bump, commit and tag:**
   ```bash
   cargo ws version minor --no-individual-tags   # or patch / major / custom X.Y.Z
   ```
   This updates `[workspace.package]` and `[workspace.dependencies]` (the
   modules follow through `workspace = true`), commits every uncommitted
   change (the changelog edit included), tags `vX.Y.Z` and pushes. The tag
   triggers `release-cli.yml`, which attaches the CLI binaries and the
   example modules to a GitHub release.

   `--no-individual-tags` matters: without it cargo-workspaces also tags
   every crate (`interstice-cli@X.Y.Z`, …), and GitHub starts no workflow
   for a push of more than three tags. If the release didn't start, run it
   by hand: Actions → release-cli → Run workflow, with the tag `vX.Y.Z` as
   input.
5. **Publish**, in dependency order:
   ```bash
   cargo publish --workspace --allow-dirty
   ```
   `--allow-dirty` is needed because the CLI's package includes the default
   modules, which git ignores. `cargo publish --workspace --dry-run
   --allow-dirty` checks everything first; it resolves the crates against
   each other, which `cargo ws publish --dry-run` can't do for a version
   that isn't on crates.io yet.

## Module ABI

`interstice_abi::ABI_VERSION` is the format modules are built for and data is
written in. A host loads only modules of its own ABI and reads only data
written under it (DESIGN §10, §10.2); there are no compatibility paths. Any
change to a host import or to the encoding raises it: every module is rebuilt
(`scripts/build_modules.sh`), stored data is deleted on the next load after
confirmation, and the changelog says so.
