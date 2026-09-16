#!/usr/bin/env bash
# Pre-release validation for the ycallr bundle (ycallr-core + ycallr-cli).
#
# Usage:
#   ./scripts/check-release.sh              # quick checks (versions, toolchain, clean git)
#   ./scripts/check-release.sh 0.1.2        # also verify Cargo.toml matches version
#   ./scripts/check-release.sh --full       # quick checks + make ci in both repos
#   ./scripts/check-release.sh --full 0.1.2

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Resolve ycallr-core / ycallr-cli paths (bundle layout or single-repo checkout).
if [[ -f "$SCRIPT_DIR/../Cargo.toml" ]] && grep -q 'name = "ycallr-core"' "$SCRIPT_DIR/../Cargo.toml"; then
  CORE="$(cd "$SCRIPT_DIR/.." && pwd)"
  CLI="$(cd "$SCRIPT_DIR/../.." && pwd)/ycallr-cli"
elif [[ -f "$SCRIPT_DIR/../Cargo.toml" ]] && grep -q 'name = "ycallr"' "$SCRIPT_DIR/../Cargo.toml"; then
  CLI="$(cd "$SCRIPT_DIR/.." && pwd)"
  CORE="$(cd "$SCRIPT_DIR/../.." && pwd)/ycallr-core"
elif [[ -d "$SCRIPT_DIR/../ycallr-core" && -d "$SCRIPT_DIR/../ycallr-cli" ]]; then
  BUNDLE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
  CORE="$BUNDLE_ROOT/ycallr-core"
  CLI="$BUNDLE_ROOT/ycallr-cli"
else
  echo "error: could not locate ycallr-core and ycallr-cli directories" >&2
  exit 1
fi

FULL=0
EXPECTED_VERSION=""

usage() {
  cat <<'EOF'
Usage: check-release.sh [--full] [VERSION]

Validates release readiness across ycallr-core and ycallr-cli.

  --full          Run `make ci` in both repositories (slow)
  VERSION         Expected semver (e.g. 0.1.2 or v0.1.2); must match both Cargo.toml files

Exits non-zero on any failed check.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --full)
      FULL=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*)
      echo "error: unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      EXPECTED_VERSION="${1#v}"
      shift
      ;;
  esac
done

failures=0

ok() {
  echo "OK: $*"
}

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

require_dir() {
  if [[ ! -d "$1" ]]; then
    fail "missing directory: $1"
    return 1
  fi
}

cargo_version() {
  local cargo_toml="$1/Cargo.toml"
  sed -n 's/^version = "\(.*\)"/\1/p' "$cargo_toml" | head -1
}

toolchain_channel() {
  local toolchain_file="$1/rust-toolchain.toml"
  sed -n 's/^channel = "\(.*\)"/\1/p' "$toolchain_file" | head -1
}

check_clean_git() {
  local repo="$1"
  local name="$2"
  if [[ ! -d "$repo/.git" ]]; then
    echo "SKIP: $name is not a git repository"
    return 0
  fi
  if [[ -n "$(git -C "$repo" status --porcelain)" ]]; then
    fail "$name has uncommitted changes"
    git -C "$repo" status --short >&2
  else
    ok "$name working tree clean"
  fi
}

echo "==> ycallr release check"
echo "    core: $CORE"
echo "    cli:  $CLI"
echo

require_dir "$CORE" || true
require_dir "$CLI" || true
if [[ $failures -gt 0 ]]; then
  exit 1
fi

core_version="$(cargo_version "$CORE")"
cli_version="$(cargo_version "$CLI")"

if [[ "$core_version" == "$cli_version" ]]; then
  ok "Cargo.toml versions match ($core_version)"
else
  fail "Cargo.toml version mismatch (core=$core_version, cli=$cli_version)"
fi

if [[ -n "$EXPECTED_VERSION" ]]; then
  if [[ "$core_version" == "$EXPECTED_VERSION" ]]; then
    ok "Cargo.toml version matches expected ($EXPECTED_VERSION)"
  else
    fail "Cargo.toml version is $core_version, expected $EXPECTED_VERSION"
  fi
fi

core_toolchain="$(toolchain_channel "$CORE")"
cli_toolchain="$(toolchain_channel "$CLI")"

if [[ -z "$core_toolchain" || -z "$cli_toolchain" ]]; then
  fail "could not read rust-toolchain.toml channel"
elif [[ "$core_toolchain" == "$cli_toolchain" ]]; then
  ok "rust-toolchain.toml channel matches ($core_toolchain)"
else
  fail "rust-toolchain.toml mismatch (core=$core_toolchain, cli=$cli_toolchain)"
fi

if diff -q "$CORE/rust-toolchain.toml" "$CLI/rust-toolchain.toml" >/dev/null; then
  ok "rust-toolchain.toml files are identical"
else
  fail "rust-toolchain.toml files differ between repos"
  diff -u "$CORE/rust-toolchain.toml" "$CLI/rust-toolchain.toml" >&2 || true
fi

for repo in "$CORE" "$CLI"; do
  workflow="$repo/.github/workflows/ci.yml"
  if [[ -f "$workflow" ]] && grep -q "RUST_TOOLCHAIN_VERSION: \"$core_toolchain\"" "$workflow"; then
    ok "CI RUST_TOOLCHAIN_VERSION matches in $(basename "$repo")"
  elif [[ -f "$workflow" ]]; then
    fail "CI RUST_TOOLCHAIN_VERSION in $(basename "$repo") does not match $core_toolchain"
  fi
done

if command -v rustc >/dev/null; then
  active="$(rustc --version | awk '{print $2}')"
  if [[ "$active" == "$core_toolchain" ]]; then
    ok "active rustc matches pinned toolchain ($active)"
  else
    fail "active rustc is $active, expected $core_toolchain (run: rustup default $core_toolchain)"
  fi
else
  echo "SKIP: rustc not installed locally"
fi

check_clean_git "$CORE" "ycallr-core"
check_clean_git "$CLI" "ycallr-cli"

echo
echo "==> ycallr-core header drift check"
(
  cd "$CORE"
  if ! command -v cbindgen >/dev/null; then
    cargo build --features ffi --locked
    cargo install cbindgen --locked
  else
    cargo build --features ffi --locked
  fi
  cbindgen --crate ycallr-core -l c -o ycallr.h.generated
  if diff -q ycallr.h ycallr.h.generated >/dev/null; then
    ok "ycallr.h matches cbindgen output"
  else
    fail "ycallr.h is out of date (run cbindgen and commit ycallr.h)"
    diff -u ycallr.h ycallr.h.generated >&2 || true
  fi
  rm -f ycallr.h.generated
)

if [[ $FULL -eq 1 ]]; then
  echo
  echo "==> ycallr-core make ci"
  (cd "$CORE" && make ci)

  echo
  echo "==> ycallr-cli make ci"
  (cd "$CLI" && make ci)
fi

echo
if [[ $failures -eq 0 ]]; then
  echo "All release checks passed."
  if [[ -n "$EXPECTED_VERSION" ]]; then
    tag="v$EXPECTED_VERSION"
    echo
    echo "Next steps:"
    echo "  1. Commit and push both repos to main"
    echo "  2. Tag both repositories with $tag"
    echo "     git -C ycallr-core tag $tag && git -C ycallr-core push origin $tag"
    echo "     git -C ycallr-cli tag $tag && git -C ycallr-cli push origin $tag"
    echo "  3. Verify GitHub Actions on both repos"
    echo "  4. Confirm artifacts: ycallr-core release (ycallr.h), ycallr-cli release (binaries + ReqPack)"
  fi
  exit 0
fi

echo "$failures check(s) failed." >&2
exit 1
