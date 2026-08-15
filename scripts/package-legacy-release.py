#!/usr/bin/env python3
"""Create one deterministic, identity-checked Legacy CLI release archive."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile


COMPATIBILITY = "shea-legacy-cli-v1"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument(
        "--source-date-epoch",
        type=int,
        default=int(os.environ.get("SOURCE_DATE_EPOCH", "0")),
    )
    return parser.parse_args()


def target_identity(target: str) -> tuple[str, str]:
    if target == "aarch64-apple-darwin":
        return "macos", "aarch64"
    if target == "x86_64-apple-darwin":
        return "macos", "x86_64"
    if target == "x86_64-unknown-linux-gnu":
        return "linux", "x86_64"
    raise SystemExit(f"unsupported first-slice Legacy release target: {target}")


def runtime_identity(binary: Path) -> dict[str, object]:
    result = subprocess.run(
        [str(binary), "--runtime-info"],
        check=True,
        capture_output=True,
        text=True,
    )
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SystemExit(f"malformed runtime identity JSON: {error}") from error


def validate(identity: dict[str, object], args: argparse.Namespace) -> None:
    platform, architecture = target_identity(args.target)
    expected = {
        "schema_version": 1,
        "binary_role": "legacy_cli",
        "source_revision": args.source_revision,
        "target": args.target,
        "platform": platform,
        "architecture": architecture,
        "compatibility": COMPATIBILITY,
    }
    failures = [
        f"{key}: expected {value!r}, found {identity.get(key)!r}"
        for key, value in expected.items()
        if identity.get(key) != value
    ]
    version = identity.get("cli_version")
    if not isinstance(version, str) or not version:
        failures.append("cli_version is missing")
    elif args.release_tag != f"legacy-v{version}":
        failures.append(
            f"release tag {args.release_tag!r} does not match cli_version {version!r}"
        )
    if failures:
        raise SystemExit("Legacy release identity rejected: " + "; ".join(failures))


def deterministic_archive(binary: Path, archive: Path, member_name: str, epoch: int) -> None:
    data = binary.read_bytes()
    info = tarfile.TarInfo(member_name)
    info.size = len(data)
    info.mode = 0o755
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.mtime = max(epoch, 0)
    with archive.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=max(epoch, 0)) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as output:
                output.addfile(info, io.BytesIO(data))


def main() -> None:
    args = parse_args()
    if not args.binary.is_file():
        raise SystemExit(f"missing Legacy binary: {args.binary}")
    identity = runtime_identity(args.binary)
    validate(identity, args)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    version = str(identity["cli_version"])
    member_name = "shea-symphony-legacy"
    archive_name = f"shea-symphony-legacy-{version}-{args.target}.tar.gz"
    archive = args.output_dir / archive_name
    deterministic_archive(args.binary, archive, member_name, args.source_date_epoch)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    identity_record = {
        **identity,
        "release_tag": args.release_tag,
        "archive": archive_name,
        "binary": member_name,
        "sha256": digest,
    }
    (args.output_dir / f"{args.target}.identity.json").write_text(
        json.dumps(identity_record, indent=2) + "\n",
        encoding="utf-8",
    )
    (args.output_dir / f"{args.target}.sha256").write_text(
        f"{digest}  {archive_name}\n",
        encoding="utf-8",
    )
    print(
        f"legacy_release_package=ok target={args.target} revision={args.source_revision} "
        f"archive={archive} sha256={digest}"
    )


if __name__ == "__main__":
    main()
