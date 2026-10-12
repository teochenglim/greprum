#!/usr/bin/env bash
# One-command release, identical in every repo (copy this file + the 3-line Makefile target):
#
#   1. commit your work           git commit -am "v0.12.0: …"
#   2. make release VERSION=0.12.0
#
# What it does (and refuses to do):
#   - VERSION must be x.y.z, the tag vX.Y.Z must not exist (locally or on origin)
#   - the working tree must be clean: you commit first, this only adds the version bump
#   - HEAD must be a NEW commit on top of the upstream branch (never amends a pushed commit,
#     never needs --force; diverged or behind => stop with instructions)
#   - RELEASE/vX.Y.Z.md must exist when the repo has a RELEASE/ directory
#   - runs `make check` unless SKIP_CHECK=1 (CI repeats it on the tag)
#   - bumps VERSION (and Cargo.toml/Cargo.lock when present) via `make bump`, amends the bump into HEAD
#   - pushes HEAD, tags vX.Y.Z, pushes the tag => CI/Release/Docker workflows run
#
# DRY_RUN=1 prints what would happen after running every check. Never forces anything.
set -euo pipefail

VERSION="${1:-${VERSION:-}}"
DRY="${DRY_RUN:-0}"
say() { printf '\033[36m==>\033[0m %s\n' "$*"; }
die() { printf '\033[31mrelease: %s\033[0m\n' "$*" >&2; exit 1; }
run() { if [ "$DRY" = 1 ]; then echo "    (dry-run) $*"; else "$@"; fi; }

[ -n "$VERSION" ] || die "usage: make release VERSION=x.y.z"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "VERSION must look like 1.2.3, got '$VERSION'"
TAG="v$VERSION"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not a git repository"
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
[ "$BRANCH" != "HEAD" ] || die "detached HEAD: check out a branch first"
UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
[ -n "$UPSTREAM" ] || die "branch '$BRANCH' has no upstream; run: git push -u origin $BRANCH"
REMOTE="${UPSTREAM%%/*}"

say "checking $BRANCH -> $UPSTREAM for $TAG"
git fetch -q "$REMOTE" --tags || die "cannot reach $REMOTE"

[ -z "$(git status --porcelain)" ] || { git status --short >&2; die "working tree is not clean: commit your changes first (this target only adds the version bump)"; }

AHEAD="$(git rev-list --count "$UPSTREAM..HEAD")"
BEHIND="$(git rev-list --count "HEAD..$UPSTREAM")"
if [ "$BEHIND" -gt 0 ]; then
  die "$BRANCH is $BEHIND commit(s) behind (or diverged from) $UPSTREAM. Integrate first: git pull --rebase $REMOTE $BRANCH"
fi
if [ "$AHEAD" -eq 0 ]; then
  die "HEAD is already on $UPSTREAM; the bump would amend a pushed commit. Make a new commit first (git commit -am \"$TAG: …\"), then re-run."
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then die "tag $TAG already exists locally"; fi
if git ls-remote --exit-code --tags "$REMOTE" "refs/tags/$TAG" >/dev/null 2>&1; then die "tag $TAG already exists on $REMOTE"; fi

if [ -d RELEASE ] && [ ! -f "RELEASE/$TAG.md" ]; then
  die "RELEASE/$TAG.md is missing: write the release notes (scope, ACs, checklist) before releasing"
fi

if [ "${SKIP_CHECK:-0}" != 1 ] && make -n check >/dev/null 2>&1; then
  say "running make check (SKIP_CHECK=1 to skip; CI re-runs it on the tag)"
  run make check
fi

say "bumping VERSION -> $VERSION"
if make -n bump VERSION="$VERSION" >/dev/null 2>&1; then
  run make bump VERSION="$VERSION"
else
  run sh -c "echo '$VERSION' > VERSION"
fi

FILES=()
for f in VERSION Cargo.toml Cargo.lock; do [ -f "$f" ] && FILES+=("$f"); done
say "amending the bump into $(git rev-parse --short HEAD): ${FILES[*]}"
run git add "${FILES[@]}"
if [ "$DRY" = 1 ]; then
  echo "    (dry-run) git commit --amend --no-edit"
else
  git commit --amend --no-edit >/dev/null
fi

say "pushing $BRANCH, tagging $TAG"
run git push "$REMOTE" HEAD
run git tag "$TAG"
run git push "$REMOTE" "$TAG"

URL="$(git remote get-url "$REMOTE" 2>/dev/null | sed -E 's#^(ssh://)?git@github.com[:/]#https://github.com/#; s#\.git$##')"
say "released $TAG — workflows: $URL/actions"
