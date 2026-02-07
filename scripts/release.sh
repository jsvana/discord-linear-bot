#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/release.sh 0.2.0
# Tags the current commit and pushes, triggering the GitHub Actions release build.

if [ $# -ne 1 ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.2.0"
    exit 1
fi

VERSION="$1"
TAG="v${VERSION}"

# Verify we're on main and clean
BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [ "$BRANCH" != "main" ]; then
    echo "Error: must be on main branch (currently on $BRANCH)"
    exit 1
fi

if ! git diff --quiet HEAD; then
    echo "Error: working tree has uncommitted changes"
    exit 1
fi

# Check tag doesn't already exist
if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "Error: tag $TAG already exists"
    exit 1
fi

echo "Tagging $TAG and pushing..."
git tag "$TAG"
git push origin "$TAG"

echo ""
echo "Release $TAG triggered. Watch the build at:"
echo "  https://github.com/jsvana/discord-linear-bot/actions"
echo ""
echo "Once complete, update Ansible:"
echo "  discord_linear_bot_version: \"$VERSION\""
