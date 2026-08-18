# Provenance Notes

## Current Evidence

The design system is grounded in current repository sources rather than checked-in repository snapshots:

- `../README.md` for the public product and runtime boundary.
- `../docs/README.md` for coding-agent context authority.
- `../app/README.md` for the App/runtime boundary.
- `../app/src/app.css` and `../app/src/lib/LaneCard.svelte` for live UI evidence.
- `../docs/milestones/2607-hardening/README.md` for the current runtime transition.

When design work needs additional repository or external evidence, capture it ephemerally for that task and cite the authoritative source. Do not commit a copied repository tree as standing context: it becomes stale, pollutes search, and competes with live sources.

## Preserved Assets

- `build/favicon.svg` and `assets/favicon.svg` are retained design-package copies of the runtime asset at `../app/static/favicon.svg`.

## Design Decisions

The generated system prefers the live cockpit CSS in `../app/src/app.css` because it is concrete implementation evidence. The preserved favicon carries the runtime asset lineage, and the token file exposes its tones as supporting asset colors rather than primary app chrome.
