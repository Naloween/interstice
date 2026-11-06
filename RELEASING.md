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
2. **The embedded modules are current.** Run `scripts/build_modules.sh` and
   commit the refreshed `.wasm` files under `crates/interstice-cli/module_*`.
   The CLI ships them, so they should be built with the SDK being released.
3. **The changelog is ready.** Rename "Unreleased" in `CHANGELOG.md` to the
   new version.
4. **Bump, commit and tag:**
   ```bash
   cargo ws version minor   # or patch / major / custom X.Y.Z
   ```
   This updates `[workspace.package]` and `[workspace.dependencies]` (the
   modules follow through `workspace = true`), commits, tags `vX.Y.Z` and
   pushes. The tag triggers `release-cli.yml`, which attaches CLI binaries to
   a GitHub release.
5. **Publish**, in dependency order:
   ```bash
   cargo ws publish --from-git
   ```

## Module ABI

`interstice_abi::ABI_VERSION` is the format modules are built for and data is
written in. A host loads only modules of its own ABI and reads only data
written under it (DESIGN §10, §10.2); there are no compatibility paths. Any
change to a host import or to the encoding raises it: every module is rebuilt
(`scripts/build_modules.sh`), stored data is deleted on the next load after
confirmation, and the changelog says so.
