# ycallr CLI

Terminal runner for [ycallr-core](https://github.com/Coditary/ycallr-core).

## Install

**Pre-built binaries:** [GitHub Releases](https://github.com/Coditary/ycallr-cli/releases) (Linux, macOS, Windows — x86_64 and aarch64).

**From source** (requires sibling `ycallr-core` repo):

```bash
cargo build --release
```

## Quick start

```bash
# Token stays in your environment — not in the YAML file
export GITHUB_TOKEN=ghp_...

ycallr install path/to/github_api.yaml
ycallr --list
ycallr github list-issues --owner=rust-lang --repo=rust
```

Example profile: `../ycallr-core/examples/github_api.yaml`

## Authentication

Profiles reference secrets via `${ENV_VAR}` placeholders. ycallr resolves them from your shell at call time:

```yaml
env:
  - name: GITHUB_TOKEN
    required: true
auth:
  github:
    type: bearer
    token: ${GITHUB_TOKEN}
```

The installed `.pb` file does **not** contain the token value.

## Commands

```bash
ycallr --help
ycallr install <path/to/profile.yaml>
ycallr import-openapi <spec.json> --preset github
ycallr <api> <command...> [--key=value] [--json] [--verbose]
```

## Configuration

- `YCALLR_CONFIG_DIR` — override profile directory (default: platform config dir + `/ycallr/apis`)
- `RUST_LOG=ycallr=debug` — debug logging (or `--verbose`)

## Development

```bash
make ci              # fmt + clippy + test + audit
make release-check   # validate release readiness
```
