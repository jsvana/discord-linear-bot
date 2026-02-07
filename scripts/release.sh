#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/release.sh 0.2.0
# Tags the current commit and pushes, triggering the GitHub Actions release build.
# Waits for the release to complete and prints the version + SHA256 checksum.

if [ $# -ne 1 ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.2.0"
    exit 1
fi

VERSION="${1#v}"
TAG="v${VERSION}"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: version must be in semver format (e.g. 0.2.0), got: $VERSION"
    exit 1
fi
REPO="jsvana/discord-linear-bot"
FILENAME="discord-linear-bot-${TAG}-x86_64-linux.tar.gz"

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
echo "Waiting for release build to complete..."

# Poll until the release exists (check every 15s, timeout after 15 minutes)
MAX_ATTEMPTS=60
for i in $(seq 1 $MAX_ATTEMPTS); do
    if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
        break
    fi
    if [ "$i" -eq "$MAX_ATTEMPTS" ]; then
        echo "Timed out waiting for release. Check manually:"
        echo "  https://github.com/$REPO/actions"
        exit 1
    fi
    printf "  waiting... (%d/%d)\r" "$i" "$MAX_ATTEMPTS"
    sleep 15
done

echo ""
echo "Release $TAG is available. Downloading binary..."

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

gh release download "$TAG" --repo "$REPO" --pattern "$FILENAME" --dir "$TMPDIR"

CHECKSUM=$(shasum -a 256 "$TMPDIR/$FILENAME" | awk '{print $1}')

echo ""
echo "=== Ansible values ==="
echo "  discord_linear_bot_version: \"$VERSION\""
echo "  discord_linear_bot_checksum: \"sha256:$CHECKSUM\""
