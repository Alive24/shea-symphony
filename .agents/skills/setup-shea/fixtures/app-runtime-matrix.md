# Stable App Runtime Matrix

## Observed input

One resolved stable tag and full commit provide the four required Release
assets. The operator runs `setup-shea` on Apple Silicon macOS or Windows x64
with one of these discovery states: compatible installed App, absent App,
stale release, wrong platform, tampered sidecar, or repeated compatible setup.
The operator may also decline or cancel the visible installation plan.

## Expected plan

- Verify `release-manifest.json`, `SHA256SUMS`, GitHub asset digests, and the
  selected native package before any persistent machine write.
- `compatible`: reuse the installed App after live identity and digest checks.
- `missing`: offer the exact confirmation-gated native App installation.
- `stale`: offer an explicit update while preserving repository files.
- `wrong_platform`: reject the package and select only the supported host asset.
- `tampered`: refuse reuse and offer a verified replacement plan.
- `declined`: discard staging and perform no install, replacement, or launch.
- `repeated`: reuse the compatible App and preserve target-owned customizations.
- macOS uses visible Gatekeeper and Applications placement; Windows uses the
  visible NSIS flow without `/S` or hidden elevation.

## Expected result

Only a discovery record whose digest, live `--runtime-info`, role,
compatibility, platform, target, version, and full release revision all match
is ready. Missing or declined installation remains blocked. Successful initial
or repeated setup reaches no-claim readiness without creating an issue,
changing lane state, or launching Main, Review, or Merge.
