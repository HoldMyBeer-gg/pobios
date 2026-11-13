#!/bin/sh
# Fetch or update the upstream Path of Building checkout the app bundles.
#
# PathOfBuilding/ is not tracked here: it is upstream's repo, ~1.8GB with
# history, and it moves per league. Run this to clone it fresh or bring it
# up to date, optionally pinning a ref:
#
#   scripts/fetch-pob.sh            # clone or fast-forward to upstream dev
#   scripts/fetch-pob.sh v2.67.2    # a release tag
set -eu
cd "$(dirname "$0")/.."
REPO=https://github.com/PathOfBuildingCommunity/PathOfBuilding.git
REF=${1:-dev}
if [ ! -d PathOfBuilding/.git ]; then
  git clone "$REPO" PathOfBuilding
fi
git -C PathOfBuilding fetch --tags origin
git -C PathOfBuilding checkout -q "$REF"
if git -C PathOfBuilding symbolic-ref -q HEAD >/dev/null; then
  git -C PathOfBuilding pull -q --ff-only
fi
echo "PathOfBuilding at $(git -C PathOfBuilding log --oneline -1)"
