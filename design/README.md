# Shea Symphony Design System

Shea Symphony Design System is a reusable Open Design package for building evidence-first cockpit UI for Shea Symphony, a supervised AI-native engineering workflow system.

## Product Overview

Shea Symphony helps a human operator turn rough engineering intent into executable issue contracts, run implementation agents in isolated workspaces, request independent agent review, preserve audit evidence, and land approved pull requests through a guarded merge lane.

The current repository describes Shea Symphony as a team workflow system for supervised AI-native engineering. It is designed around a human operator, not a hidden daemon. The primary product surfaces are:

- Issue Forge for shaping rough work into executable issues.
- Operator Desk for the top human decisions and lane readiness.
- Main lane for implementation work.
- Agent Review and Human Review for independent review and approval boundaries.
- Merge lane for guarded PR landing, repair, retry, or escalation.
- Events, workpads, Doctor diagnostics, and timeline evidence for auditability.

## Product Context

Shea Symphony is transitioning from its protected 2606 Legacy runtime to a Temporal-backed runtime while retaining one operator workflow contract. The current App, repository context router, milestone package, source code, and tests are the evidence for product behavior; this design package does not preserve copies of them.

Source references:

- GitHub repository: `https://github.com/Alive24/shea-symphony`.
- Repository context router: `../docs/README.md`.
- App/runtime boundary: `../app/README.md`.
- Live cockpit CSS: `../app/src/app.css`.
- Live lane component: `../app/src/lib/LaneCard.svelte`.
- Current runtime milestone: `../docs/milestones/2607-hardening/README.md`.

## Package Contents

- `DESIGN.md` - canonical design rules: product context, foundations, components, motion, voice, and anti-patterns.
- `colors_and_type.css` - reusable source-backed color, typography, spacing, radius, elevation, and motion tokens, with Daylight as the default theme and Night as the source cockpit mode.
- `SKILL.md` - Claude Design-style skill entry for future agents.
- `preview/` - focused Design System tab review cards.
- `ui_kits/app/` - runnable React/Babel app kit that composes the Operator Desk surface from modular components.
- `assets/favicon.svg` - convenience alias for the runtime favicon.
- `build/favicon.svg` - design-package copy of the current runtime favicon.
- `PROVENANCE.md` - current source anchors and the rule against checked-in repository snapshots.

## Preview Manifest

- `preview/colors-primary.html` - Inspect Starbucks palette roles and their Daylight/Night application: historic green brand signal, luminous green CTA, House Green cockpit surface, cream/ceramic neutrals, reserved gold, text ladders, and semantic colors.
- `preview/colors-theme-modes.html` - Inspect Daylight default tokens beside the Night cockpit tokens; reviewers should confirm previews load in Daylight while Night remains available through `data-theme="night"` or `.ss-theme-night`.
- `preview/colors-status.html` - Inspect success, warning, danger, and accent status treatment in both Daylight and Night for readiness, review, merge, and unsafe continuation states.
- `preview/typography-specimens.html` - Inspect display/body/mono hierarchy in both themes, including operational metadata usage for issue numbers, backend/session identifiers, commands, and review status.
- `preview/spacing-tokens.html` - Inspect the 4/8/12/16/20/24/32/48 spacing scale in both themes as used by cards, navigation bands, command forms, and workspace gutters.
- `preview/spacing-radius.html` - Inspect restrained 4px controls, 8px cards/panels, and pill status controls in both themes.
- `preview/spacing-shadows.html` - Inspect flat, ring, and raised elevation treatments in both themes sourced from the cockpit CSS.
- `preview/components-buttons.html` - Inspect primary/secondary/danger buttons, compact lane actions, disabled state, and segmented mode controls in both themes, modeled from the source app CSS.
- `preview/components-inputs.html` - Inspect command-console input fields, textarea, focus ring, and lane/status select behavior in both themes.
- `preview/components-lanes.html` - Inspect source-backed LaneCard structure in both themes: posture, lane name, compact View lane action, active/retrying/blocked metrics, and latest evidence with breathable vertical spacing.
- `preview/brand-assets.html` - Inspect preserved runtime assets loaded with real `<img>` references from `build/favicon.svg` and `assets/favicon.svg` on both Daylight and Night surfaces.

Keep this manifest synchronized with the actual `preview/*.html` files whenever previews change.

## Reuse Workflow

1. Read `DESIGN.md` first to understand product boundaries and lane authority.
2. Load `colors_and_type.css` into the artifact before component or page CSS.
3. Use Daylight by default for package review and handoff screens. Add `data-theme="night"` to the document root only when the artifact should match the original dark cockpit.
4. Inspect the focused cards under `preview/` for token and component behavior; every preview module includes Daylight and Night samples and should stay that way when edited.
5. Use `ui_kits/app/index.html` and its `components/` files as the applied Operator Desk starting point.
6. Inspect `../app/src/lib/LaneCard.svelte` when implementing lane summaries in code-adjacent artifacts.
7. Preserve `build/favicon.svg` if generating app shells, launcher views, or brand asset cards.

## Implementation Notes

Shea Symphony web prototypes should use SvelteKit with Vite when creating production-like web app scaffolds. For lightweight Open Design examples, the included React/Babel UI kit is acceptable because it is self-contained and reviewable.

The main Operator Desk should answer what needs human attention right now. It should show only the top 1 to 3 human tasks as large triage cards. Worker details and lane internals belong on secondary lane pages.

Daylight is the package default for previews and UI kit review. Night uses the same Starbucks palette shifted onto House Green surfaces and should be used when production-like operator ambience is more important than bright review legibility.

## Anti-pattern Reminder

Avoid purple AI gradients, dense monitoring walls, tiny terminal aesthetics, mascot/fantasy imagery, chat bubbles as the dominant metaphor, fake metrics, and UI that hides the next safe operator action.
