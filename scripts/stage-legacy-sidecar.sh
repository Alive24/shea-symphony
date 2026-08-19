#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
target=${SHEA_LEGACY_SIDECAR_TARGET:-}
mode=stage

if [ "${1:-}" = "--check" ]; then
  mode=check
elif [ -n "${1:-}" ]; then
  echo "usage: scripts/stage-legacy-sidecar.sh [--check]" >&2
  exit 2
fi

if [ -z "$target" ]; then
  target=$(rustc -vV | sed -n 's/^host: //p')
fi
if [ -z "$target" ]; then
  echo "could not resolve the Rust target triple" >&2
  exit 1
fi

case "$target" in
  *windows*) executable_suffix=.exe ;;
  *) executable_suffix= ;;
esac

source_binary="$repository_root/target/$target/release/shea-symphony-legacy$executable_suffix"
staging_dir="$repository_root/app/src-tauri/binaries"
staged_binary="$staging_dir/shea-symphony-legacy-$target$executable_suffix"

if [ "$mode" = check ]; then
  if [ ! -f "$staged_binary" ]; then
    echo "missing target-specific Legacy sidecar for $target: $staged_binary" >&2
    exit 1
  fi
  echo "legacy_sidecar_check=ok target=$target path=$staged_binary"
  exit 0
fi

cargo build \
  --locked \
  --release \
  --manifest-path "$repository_root/Cargo.toml" \
  --bin shea-symphony-legacy \
  --target "$target"

if [ ! -f "$source_binary" ]; then
  echo "Legacy build completed without expected target artifact: $source_binary" >&2
  exit 1
fi

mkdir -p "$staging_dir"
temporary_binary="$staged_binary.tmp.$$"
trap 'rm -f "$temporary_binary"' EXIT HUP INT TERM
cp "$source_binary" "$temporary_binary"
chmod 755 "$temporary_binary"
mv "$temporary_binary" "$staged_binary"
trap - EXIT HUP INT TERM

runtime_json=$($staged_binary --runtime-info)
expected_revision=$(git -C "$repository_root" rev-parse HEAD)
RUNTIME_JSON="$runtime_json" EXPECTED_REVISION="$expected_revision" EXPECTED_TARGET="$target" node -e '
  const identity = JSON.parse(process.env.RUNTIME_JSON);
  if (identity.schema_version !== 1 || identity.binary_role !== "legacy_cli") {
    throw new Error("staged artifact is not a marked Legacy CLI");
  }
  if (identity.compatibility !== "shea-legacy-cli-v1") {
    throw new Error("staged artifact has an incompatible Legacy contract");
  }
  if (identity.source_revision !== process.env.EXPECTED_REVISION) {
    throw new Error("staged artifact source revision does not match the App checkout");
  }
  if (identity.target !== process.env.EXPECTED_TARGET) {
    throw new Error("staged artifact target does not match the requested App target");
  }
  const expected = process.env.EXPECTED_TARGET === "aarch64-apple-darwin"
    ? { platform: "macos", architecture: "aarch64" }
    : process.env.EXPECTED_TARGET === "x86_64-pc-windows-msvc"
      ? { platform: "windows", architecture: "x86_64" }
      : null;
  if (expected && (identity.platform !== expected.platform || identity.architecture !== expected.architecture)) {
    throw new Error("staged artifact platform or architecture does not match the requested App target");
  }
'

echo "legacy_sidecar_stage=ok target=$target revision=$expected_revision path=$staged_binary"
