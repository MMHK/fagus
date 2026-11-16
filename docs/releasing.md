# Releasing

A release consists of the `fagus` binary for every platform, attached to a GitHub release of `nforst/fagus`, and the Composer package `nforst/fagus`, which installs the binary of its release and accepts no other: `php/fagus/src/Internal/Release.php` lists the SHA-256 of each binary.

1. **Prepare.** Set the version in `Cargo.toml` (`[workspace.package]`), move the entries under `[Unreleased]` in `CHANGELOG.md` to the new version, name a new minor version as the supported one in `SECURITY.md`, run `tools/notices.sh` (it rewrites `THIRD-PARTY-NOTICES` with the licenses of the crates the binary is built from; it needs `cargo install --locked cargo-about --features cli`) and commit.
2. **Build.** Run the workflow *Release* (`.github/workflows/release.yml`) on that commit with the version, for example `gh workflow run release.yml -f version=0.1.0`. It builds the binaries for Linux (x64 and arm64, glibc 2.17 and musl), macOS (x64 and arm64) and Windows (x64), checks that those the runners can run render a document, and attaches them with their sums (`SHA256SUMS`) to a draft release `v0.1.0`.
3. **Pin.** Fetch the sums and write them into the PHP package:

   ```sh
   gh release download v0.1.0 -p SHA256SUMS -D /tmp/fagus-0.1.0
   php tools/pin-binaries.php 0.1.0 /tmp/fagus-0.1.0/SHA256SUMS
   git commit -am "chore: pin the binaries of 0.1.0"
   ```

   This commit changes nothing but `Release.php`, so the binaries are those of the code it is tagged with.
4. **Check.** Install the binary as a user would, from the files of the draft: GitHub does not hand out a draft by its tag, so fetch them (`gh release download v0.1.0 -D /tmp/fagus-0.1.0`) and set `FAGUS_DOWNLOAD_URL=/tmp/fagus-0.1.0`, which the installer checks against the pinned sums like a download. Then run the corpus checks: `tools/validate.sh` and the comparison with Chrome.
5. **Tag and publish.** Tag the pin commit `v0.1.0`, push the tag, then publish the draft release, which takes the existing tag. The Composer package is released with the same tag: Packagist reads `composer.json` at the root of this repository.

## Composer

The Composer package `nforst/fagus` is this repository: `composer.json` at the root, the PHP code in `php/fagus` (`Fagus\`) and `php/laravel` (`Fagus\Laravel\`, registered by Laravel's package discovery). `.gitattributes` leaves the engine's sources, tests and tools out of the archives Composer downloads, so an installation holds only the PHP code and the binary.

Packagist lists the package from `https://github.com/nforst/fagus` and updates it on every push and tag through GitHub's webhook. Before a release is tagged, a project can try a branch through a `vcs` repository:

```json
"repositories": [{ "type": "vcs", "url": "https://github.com/nforst/fagus" }]
```

A development version has no pinned binaries, so the plugin installs none; set `FAGUS_BINARY` to a binary built from the same commit.
