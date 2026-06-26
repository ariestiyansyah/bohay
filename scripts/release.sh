#!/usr/bin/env bash
#
# release.sh — cut a new bohay version to crates.io, GitHub (binaries), and Homebrew.
#
#   scripts/release.sh 0.1.1             # full release (prompts before publishing)
#   scripts/release.sh 0.1.1 --dry-run   # bump + verify only, then revert — no release
#   scripts/release.sh 0.1.1 --yes       # skip the confirmation prompt
#
# Prereqs:  `cargo login` done · `gh auth login` · push access to the repo.
# Tap:      the Homebrew formula in ./homebrew-bohay (or $BOHAY_TAP_DIR) — the real
#           `brew install RizRiyz/bohay/bohay` source — is bumped & pushed too.
set -euo pipefail

REPO="RizRiyz/bohay"

die()  { printf '\033[31merror:\033[0m %s\n' "$1" >&2; exit 1; }
step() { printf '\n\033[36m▸ %s\033[0m\n' "$1"; }
sha256() { if command -v shasum >/dev/null; then shasum -a 256; else sha256sum; fi | cut -d' ' -f1; }
# Rewrite a formula's release url + sha256 in place ($TAG/$SHA set before calling).
bump_formula() {
  perl -0pi -e "s{archive/refs/tags/v[0-9.]+\.tar\.gz}{archive/refs/tags/$TAG.tar.gz}g" "$1"
  perl -0pi -e "s/^  sha256 \"[0-9a-f]{64}\"/  sha256 \"$SHA\"/m" "$1"
}

VERSION="${1:-}"
MODE="${2:-}"
[ -n "$VERSION" ] || die "usage: scripts/release.sh X.Y.Z [--dry-run|--yes]"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must be semver X.Y.Z (got '$VERSION')"
TAG="v$VERSION"
cd "$(git rev-parse --show-toplevel)"

step "Preconditions"
[ "$(git branch --show-current)" = "main" ] || die "not on main"
[ -z "$(git status --porcelain)" ] || die "working tree is dirty — commit or stash first"
git fetch --tags --quiet
git rev-parse "$TAG" >/dev/null 2>&1 && die "$TAG already exists"
CURRENT=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
echo "  $CURRENT  →  $VERSION"
# The Homebrew tap (its own git repo): the in-repo clone by default.
TAP="${BOHAY_TAP_DIR:-homebrew-bohay}"
if [ -f "$TAP/Formula/bohay.rb" ]; then
  [ -z "$(git -C "$TAP" status --porcelain)" ] || die "tap '$TAP' has uncommitted changes"
  echo "  tap: $TAP  (will bump + push)"
else
  echo "  tap: none at '$TAP' — Homebrew step will print manual instructions"
fi

step "Bump Cargo.toml + Cargo.lock"
# Only the [package] version is at the start of a line; deps use `name = "..."`.
perl -0pi -e "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"$VERSION\"/m" Cargo.toml
cargo check --quiet                       # syncs Cargo.lock's bohay version
grep -q "^version = \"$VERSION\"" Cargo.toml || die "Cargo.toml bump failed"

step "Verify (fmt · clippy · test · publish dry-run)"
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo publish --dry-run

step "Release notes preview (what the workflow will publish on the GitHub Release)"
bash scripts/changelog.sh "$TAG"

if [ "$MODE" = "--dry-run" ]; then
  git checkout -- Cargo.toml Cargo.lock
  step "Dry run OK — everything passed. Re-run without --dry-run to release."
  exit 0
fi

if [ "$MODE" != "--yes" ]; then
  printf "\nRelease \033[1m%s\033[0m to crates.io + GitHub + Homebrew. Continue? [y/N] " "$TAG"
  read -r ans
  [ "$ans" = "y" ] || [ "$ans" = "Y" ] || { git checkout -- Cargo.toml Cargo.lock; die "aborted"; }
fi

step "Commit + tag"
git add Cargo.toml Cargo.lock
git commit -m "release: $TAG"
git tag -a "$TAG" -m "$TAG"

step "Push (triggers the release workflow → binaries)"
git push origin main
git push origin "$TAG"

step "Publish to crates.io"
cargo publish

step "Homebrew formula (source tarball is ready the instant the tag is pushed)"
TARBALL="https://github.com/$REPO/archive/refs/tags/$TAG.tar.gz"
SHA=$(curl -fsSL --retry 5 --retry-delay 2 "$TARBALL" | sha256)
[ -n "$SHA" ] || die "could not fetch + hash $TARBALL"
echo "  sha256: $SHA"

# Keep the in-repo reference copy current (committed to main).
bump_formula Formula/bohay.rb
git add Formula/bohay.rb
git commit -m "homebrew: $TAG"
git push origin main

# Bump the real tap (its own repo) — this is what `brew install` pulls.
if [ -f "$TAP/Formula/bohay.rb" ]; then
  step "Update tap ($TAP)"
  bump_formula "$TAP/Formula/bohay.rb"
  git -C "$TAP" add Formula/bohay.rb
  git -C "$TAP" commit -m "bohay $TAG"
  git -C "$TAP" push
  echo "  ✓ tap pushed — brew install $REPO/bohay now serves $TAG"
else
  step "Tap '$TAP' not found — finish Homebrew by hand:"
  echo "    git clone git@github.com:${REPO%%/*}/homebrew-bohay.git"
  echo "    # in it: set url → .../$TAG.tar.gz and sha256 → $SHA, then commit & push"
fi

step "Done — $TAG released 🎉"
echo "  cargo:    cargo install bohay"
echo "  binaries: https://github.com/$REPO/releases/tag/$TAG  (workflow building now)"
echo "  brew:     brew install $REPO/bohay"
