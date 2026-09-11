#!/bin/sh
# Seal a locally built macOS bundle; no Developer ID or notarization is claimed.
set -eu
[ "$(uname -s)" = Darwin ] || exit 0
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
target_dir=${CARGO_TARGET_DIR:-$repository_root/app/src-tauri/target}
bundle=${1:-$target_dir/release/bundle/macos/Shea Symphony App.app}
if [ ! -d "$bundle" ]; then
  echo "local App bundle is missing: $bundle" >&2
  exit 1
fi
# Preserve an existing valid seal, including one made with a developer identity.
if /usr/bin/codesign --verify --deep --strict "$bundle" 2>/dev/null; then
  echo "local_bundle_seal=verified path=$bundle"
  exit 0
fi
/usr/bin/codesign --force --deep --sign - "$bundle"
/usr/bin/codesign --verify --deep --strict "$bundle"
echo "local_bundle_seal=ad_hoc_verified path=$bundle"
