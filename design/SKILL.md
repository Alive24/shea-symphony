---
name: shea-symphony-design-system
description: Use this design system to build Shea Symphony cockpit UI, previews, decks, and app prototypes grounded in source evidence from the Shea Symphony repository.
user-invocable: true
---

# Shea Symphony Design System Skill

This is a reusable Claude Design-style skill package. It includes What is inside, Source context, When to use, How to use, and design-system highlights grounded in the Shea Symphony source evidence.

## What's inside

This package contains:

- `README.md` for package context, preview manifest, source references, and reuse workflow.
- `DESIGN.md` for canonical visual and product rules.
- `colors_and_type.css` for concrete source-backed tokens, with Daylight as the default review theme and Night as the source cockpit theme.
- `preview/` for focused review cards covering colors, type, spacing, radius, elevation, components, lanes, and brand assets.
- `assets/` for convenience brand/runtime asset aliases.
- `build/` for design-package copies of runtime assets.
- `fonts/` when the design package needs distributable brand font files; this package currently uses system source stacks from `app/src/app.css`.
- `ui_kits/app/` for a runnable applied Operator Desk kit.
- `PROVENANCE.md` for current source anchors and recapture policy.

## Source Context

The system is backed by current repository sources:

- GitHub repository: `https://github.com/Alive24/shea-symphony`.
- Repository context router: `../docs/README.md`.
- App/runtime boundary: `../app/README.md`.
- Live cockpit CSS: `app/src/app.css`.
- Live lane component: `../app/src/lib/LaneCard.svelte`.
- Current runtime milestone: `../docs/milestones/2607-hardening/README.md`.

## When to use this skill

Use this skill when creating or revising Shea Symphony:

- Operator Desk surfaces.
- Lane summaries and lane detail screens.
- Human Review, Agent Review, Merge, Rework, and Need Human Input decision flows.
- Evidence timelines, workpad views, Doctor diagnostics, and command consoles.
- Design-system previews or implementation-ready UI kits for the same product family.

## How to use

1. Read `README.md` for source context and the current preview manifest.
2. Read `DESIGN.md` for rules, authority boundaries, components, voice, and anti-patterns.
3. Import or paste `colors_and_type.css` before writing artifact-specific styles.
4. Use Daylight by default for previews and design-system review. Add `data-theme="night"` or `.ss-theme-night` when a generated artifact should use the House Green cockpit mode.
5. Inspect relevant `preview/*.html` cards before designing token or component variants. Every preview card shows Daylight and Night samples; preserve that paired-theme review pattern when adding or revising modules.
6. Inspect `assets/` and `build/` before using brand or runtime imagery; use `build/favicon.svg` for app/runtime icon references.
7. Inspect `fonts/` if it exists in a future package version; this package currently binds source system font stacks only.
8. Inspect the current App source when implementing lane cards or app-shell patterns near source code.
9. Use `ui_kits/app/index.html` and `ui_kits/app/components/` as the applied product starting point.
10. Preserve references to `assets/`, `build/`, and `fonts/` when present in generated package docs.
11. Keep Main, Review, Human Review, Merge, Rework, and Need Human Input semantically separate in UI and copy.

## Highlights

- Daylight default canvas for package review uses the Starbucks palette: Neutral Warm canvas, white cards, Starbucks Green brand signal, Green Accent filled CTA, and House Green authority surfaces.
- Use `--ss-accent` (`#006241`) for issue/status signal and brand emphasis; use `--ss-action-bg` (`#00754A`) plus `--ss-action-on` for high-contrast primary buttons. Night keeps the same brand palette on House Green surfaces.
- Editorial but practical display typography, readable body type, and precise mono metadata from the source font stacks.
- Operator Desk first, sticky top navbar in the current UI kit, and secondary lane pages for worker internals. Use the source `.app-chrome` / `.workspace` density and responsive rules from `app/src/app.css`, but keep `ui_kits/app` navigation at the top unless the user explicitly asks for the older rail.
- Human triage cards should expose issue, current state, latest evidence, recommendation, and one next action, following `.attention-card`, `.issue-tag`, `.recommendation`, and `.evidence-preview`.
- Lane cards follow `../app/src/lib/LaneCard.svelte`: posture, name, compact View lane action, active/retrying/blocked metrics, latest evidence. Use `.ss-button-compact` for secondary lane/evidence actions so summary cards do not crowd.
- Command consoles follow `.command-console`, `.command-form`, `.command-preview`, `.command-output`, and `.field`.
- Motion is calm: 150ms press feedback and 200ms color/border transitions from the source motion tokens.
- Voice is evidence-backed and operator-safe, following the repository workflow contracts. Never sell automation as magic.
