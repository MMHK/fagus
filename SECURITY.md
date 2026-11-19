# Security policy

Fagus renders HTML and CSS that may come from untrusted sources. Security reports are taken seriously.

## Supported versions

Security fixes go into the latest release. Before 1.0 that is the newest minor version (0.2.x at the moment); there are no fixes for older ones.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through GitHub: [report a vulnerability](https://github.com/nforst/fagus/security/advisories/new) (the *Security* tab of the repository). If you cannot use GitHub, send the report to kontakt@niklasforst.de.

Include a description, the affected version and, if possible, a document that reproduces the problem. You will get an answer within a week. Once a fix is released, the report is published as a security advisory, crediting you if you wish.

## Scope

In scope: code execution, reading files or network resources that the caller did not allow, crashes and unbounded resource use caused by input documents, and incorrect output in PDF/A or PDF/UA mode that a validator rejects.

## Verify a binary

The release workflow builds the binaries on GitHub Actions and attests each of them: GitHub
signs, through Sigstore and without a key of ours, which workflow run of which commit built a
file with a given SHA-256. Each release also holds `SHA256SUMS` and a CycloneDX SBOM of every
binary (`fagus-<target>.cdx.json`), which lists the crates it is built from.

```sh
gh release download v1.0.0 -p fagus-x86_64-unknown-linux-gnu -p SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify fagus-x86_64-unknown-linux-gnu --repo nforst/fagus \
  --signer-workflow nforst/fagus/.github/workflows/release.yml
```

The Composer package goes further: it installs only the binary of its own release whose SHA-256
it lists (`php/fagus/src/Internal/Release.php`), and those sums are written after the
attestations were checked ([releasing.md](docs/releasing.md)).
