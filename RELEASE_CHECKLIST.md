# Release checklist

ycallr ships as **two coordinated repositories**: [ycallr-core](https://github.com/Coditary/ycallr-core) and [ycallr-cli](https://github.com/Coditary/ycallr-cli). Both must share the same semver and git tag (`v*`).

## Toolchain

Rust is pinned in `rust-toolchain.toml` (currently **1.98.0** with `rustfmt` + `clippy`). Both repos must keep identical files.

```bash
rustc --version    # should match rust-toolchain.toml
```

Install locally:

```bash
rustup toolchain install 1.98.0 -c rustfmt -c clippy
```

## Pre-release validation

From the bundle root (`ycallr/`):

```bash
chmod +x scripts/check-release.sh   # once
./scripts/check-release.sh          # quick checks
./scripts/check-release.sh 0.1.2    # verify version bump
./scripts/check-release.sh --full   # quick + make ci in both repos
```

Or via Makefile:

```bash
make release-check
make release-check-full VERSION=0.1.2
```

The script verifies:

- `Cargo.toml` versions match in core and CLI
- `rust-toolchain.toml` is identical and matches active `rustc`
- Clean git working trees (if repos are git checkouts)
- `ycallr.h` matches cbindgen output
- Optionally runs `make ci` in both repos (`--full`)

## Release steps

### 1. Prepare version bump

Update `version` in both:

- `ycallr-core/Cargo.toml`
- `ycallr-cli/Cargo.toml`

Run checks:

```bash
./scripts/check-release.sh --full 0.1.2
```

Commit and push **both** repos to `main`.

### 2. Tag and push (both repos)

Use the **same tag** on both repositories:

```bash
TAG=v0.1.2

git -C ycallr-core tag "$TAG"
git -C ycallr-core push origin "$TAG"

git -C ycallr-cli tag "$TAG"
git -C ycallr-cli push origin "$TAG"
```

Pushing `main` alone does **not** publish release artifacts.

### 3. CI artifacts

| Repository | Trigger | Artifacts |
|------------|---------|-----------|
| ycallr-core | tag `v*` | `ycallr.h` (C FFI header) |
| ycallr-cli | tag `v*` | `.tar.gz` / `.zip` binaries (6 targets), ReqPack `.rqp` + `index.json` (Linux/macOS) |

On tag builds, ycallr-cli CI checks out ycallr-core at the **matching tag ref**.

### 4. Post-release verification

- [ ] [ycallr-core releases](https://github.com/Coditary/ycallr-core/releases) contains `ycallr.h`
- [ ] [ycallr-cli releases](https://github.com/Coditary/ycallr-cli/releases) contains all platform archives
- [ ] `ycallr --version` on a downloaded binary shows the new version
- [ ] ReqPack `index.json` lists the new `.rqp` packages (if applicable)

## Bumping the Rust toolchain

1. Update `rust-toolchain.toml` in **both** repos (same `channel` and `components`)
2. Run `make ci` in core and CLI locally
3. Commit both files together with the version bump or in a dedicated PR

## Related docs

- `ycallr-core.wiki/Releases.md` — core CI and header release
- `ycallr-cli.wiki/CI-and-Releases.md` — CLI matrix builds and ReqPack
