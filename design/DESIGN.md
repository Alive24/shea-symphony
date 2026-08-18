# Shea Symphony Design System

> Category: Custom
> Surface: web
> Source evidence: `context/source-context.md`, `context/github/Alive24-shea-symphony.md`, `context/local-code/shea-symphony.md`, and snapshots under `context/local-code/shea-symphony/files/`.

Shea Symphony is a supervised AI-native engineering workflow cockpit for teams that run coding agents against tracker-backed work. It turns rough engineering intent into executable issue contracts, runs Main, Review, Human Review, and Merge lanes in isolated workspaces, records durable workpad and timeline evidence, and gives human operators clear review, recovery, and merge decisions.

## Product Context

Source-backed product context comes from the captured repository README, `docs/bootstrap/SHEA_SYMPHONY_SPEC.md`, `docs/bootstrap/SHEA_WORKFLOW.md`, and the current `docs/README.md` context router. The product is a private-first team harness for orchestrating coding agents against tracked engineering work. It preserves the official Symphony workflow shape while adding GitHub Project v2, Linear tracker support, assignee filtering, Issue Forge, issue quality gates, independent Agent Review, Human Review, guarded Merge, and Doctor diagnostics.

Primary UI surfaces evidenced by the source package:

- Operator Desk: the human-facing cockpit that answers what needs attention now.
- Issue Forge: upstream issue creation, clarification, validation, and repair.
- Main lane: implementation worker flow and PR handoff.
- Agent Review: independent review gate before Human Review.
- Human Review: human approval or Rework decision, with evidence visible.
- Merge lane: guarded merge, repair, retry, or Need Human Input escalation.
- Doctor and Events: diagnostic evidence, timeline comments, runtime/session health, and Project invariant checks.

Implementation source context:

- `app/package.json` identifies the web stack as SvelteKit, Vite, and Svelte.
- `app/src/app.css` supplies the live cockpit visual system and component classes.
- `web/.svelte-kit/output/server/chunks/LaneCard.js` supplies the captured lane summary component structure.
- `app/build/favicon.svg` supplies the preserved runtime icon asset.

## 1. Visual Theme & Atmosphere

Shea Symphony should feel like a calm human operator cockpit, not a decorative SaaS dashboard or AI chat app. The visual system is evidence-first: issue contracts, lane claims, workpads, review ledgers, PR links, runtime status, Doctor diagnostics, and state transitions are treated as first-class objects.

The interaction structure is grounded in `app/src/app.css`: spacious cockpit panels, compact status pills, lane cards, command consoles, drawers, and responsive navigation behavior. The reusable UI kit uses a top navbar for review clarity. The color system now intentionally uses the Starbucks palette requested in review: historic green, luminous CTA green, House Green, cream/ceramic surfaces, reserved gold ceremony accents, and black/white text ladders. Daylight is the default presentation mode for previews and UI kits; Night uses House Green as the production-like cockpit foundation.

Personality:

- Operator-led, supervised, and recoverable.
- Premium operator cockpit, not terminal cosplay; Daylight is warm, crisp, and reviewable, Night is House Green and production-like.
- Clear authority boundaries between Main, Review, Human Review, Merge, Rework, and Need Human Input.
- Human triage first: the main screen answers what needs human attention right now.

Avoid purple AI gradients, chat bubbles as the primary metaphor, vague automation magic, fake vanity metrics, and any UI that hides the next safe operator action.

## 2. Color

Use the Starbucks palette as the authoritative color system. Daylight must be the default in generated design-system review cards and reusable package examples. Night is not a separate brand; it is the same palette shifted onto House Green surfaces for cockpit work.

```css
:root,
:root[data-theme="daylight"],
.ss-theme-daylight {
  color-scheme: light;
  --ss-theme-name: "Daylight";
  --ss-bg: #f2f0eb;
  --ss-surface: #ffffff;
  --ss-surface-warm: #edebe9;
  --ss-surface-cool: #f9f9f9;
  --ss-fg: rgba(0, 0, 0, 0.87);
  --ss-fg-2: #006241;
  --ss-muted: rgba(0, 0, 0, 0.58);
  --ss-border: rgba(0, 0, 0, 0.18);
  --ss-border-soft: rgba(0, 0, 0, 0.10);
  --ss-accent: #006241;
  --ss-accent-on: #ffffff;
  --ss-action-bg: #00754a;
  --ss-action-on: #ffffff;
  --ss-success: #00754a;
  --ss-warn: #fbbc05;
  --ss-danger: #c82014;
}

:root[data-theme="night"],
.ss-theme-night {
  color-scheme: dark;
  --ss-theme-name: "Night";
  --ss-bg: #1e3932;
  --ss-surface: #243f37;
  --ss-surface-warm: #33433d;
  --ss-fg: rgba(255, 255, 255, 1);
  --ss-fg-2: #ffffff;
  --ss-muted: rgba(255, 255, 255, 0.70);
  --ss-border: rgba(255, 255, 255, 0.20);
  --ss-border-soft: rgba(255, 255, 255, 0.12);
  --ss-accent: #d4e9e2;
  --ss-accent-on: #1e3932;
  --ss-action-bg: #00754a;
  --ss-action-on: #ffffff;
  --ss-success: #d4e9e2;
  --ss-warn: #cba258;
  --ss-danger: #ff7a70;
}
```

Semantic roles:

- Primary: Starbucks Green `#006241` is the dominant brand signal for headings, issue tags, selected controls, and any single-color brand moment.
- CTA: Green Accent `#00754A` is the filled primary action color.
- Dark surface: House Green `#1E3932` anchors Night mode, footer-like surfaces, and high-authority cockpit bands.
- Secondary green: Green Uplift `#2b5148` is used sparingly for decorative accents or dark raised surfaces. Green Light `#d4e9e2` is for valid-state tints and light utility surfaces.
- Rewards ceremony: Gold `#cba258`, Gold Light `#dfc49d`, and Gold Lightest `#faf6ee` are reserved for premium/rewards-style callouts, not general UI chrome.
- Background: Daylight uses Neutral Warm `#f2f0eb`; do not use structural gradients.
- Primary surface: `--ss-surface` for panels, cards, command consoles, drawers.
- Warm/raised surface: Ceramic `#edebe9` and Neutral Cool `#f9f9f9` for quiet utility zones.
- Text: `--ss-fg` is `rgba(0,0,0,0.87)` on light surfaces, `--ss-muted` is `rgba(0,0,0,0.58)`, and dark surfaces use white plus `rgba(255,255,255,0.70)`.
- Border: `--ss-border-soft` for most panels; `--ss-border` for active inputs, navigation separators, and contained controls.
- Status: success uses Green Accent, warning uses Yellow `#fbbc05`, and danger uses Red `#c82014`.

Use the accent at most twice per major surface. Daylight should read as a warm cream operational workspace with precise green signal. Night should read as a House Green cockpit. Every design-system preview module should include paired Daylight and Night samples, even when the module is not a color card, so reviewers can verify the component behavior in both modes from the right-side Design System tab.

## 3. Typography

Source CSS defines:

```css
--ss-font-display: "SF Pro Display", "Inter", "Helvetica Neue", Helvetica, Arial, sans-serif;
--ss-font-body: "SF Pro Text", "Inter", "Helvetica Neue", Helvetica, Arial, sans-serif;
--ss-font-mono: ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Monaco, Consolas, monospace;
```

Type scale:

- 58px display: rare route hero or overview statement.
- 45px large heading: primary screen title in roomy contexts.
- 32px section heading: card groups, panel titles, lane overview.
- 24px card title / topbar title.
- 19px emphasized row title.
- 16px body.
- 14px compact controls and metadata.
- 13px mini labels, timestamps, command metadata.

Use mono for issue numbers, run ids, backend/session identifiers, timestamps, commands, PR refs, recovery keys, and dry-run/write-mode output. Use uppercase mini labels sparingly with `0.1em` tracking.

## 4. Spacing

Base spacing tokens:

- `--ss-space-1: 4px`
- `--ss-space-2: 8px`
- `--ss-space-3: 12px`
- `--ss-space-4: 16px`
- `--ss-space-5: 20px`
- `--ss-space-6: 24px`
- `--ss-space-8: 32px`
- `--ss-space-12: 48px`

Layout gutters:

- Desktop workspace padding: 24px to 40px.
- Tablet gutter: 24px.
- Phone gutter: 16px.
- Main surfaces use 20px to 24px inner padding.
- Dense metadata cards can use 12px to 16px.

Radius:

- Small controls: 4px.
- Panels/cards: 8px.
- Pills and segmented controls: 9999px.
- Avoid oversized soft SaaS cards; this system should feel engineered and precise.

Elevation:

- Prefer borders and subtle inset highlights.
- Raised panels may use the source `--ss-elev-raised`: inset top highlight plus deep black shadow.
- Do not use floating decorative shadows for every card.

## 5. Layout & Composition

Primary desktop composition:

- Primary navigation may use the source 232px rail or the current reusable UI kit's sticky top navbar. In package previews and `ui_kits/app`, place the navbar at the top so Daylight review starts with brand, navigation, and compact runtime status in one horizontal band.
- Main workspace with `max-width: 1560px`.
- Topbar below navigation with the current screen title and tiny readiness pills for Canonical checkout, Doctor, Auth, and Backend health.
- Operator Desk main screen focused on the top 1 to 3 human triage cards.
- Secondary lane pages for lane internals, defaulting to 5 worker cards per page.

Navigation model:

- Operator Desk
- Lanes
- Events
- Settings

Main lane summaries should include Main, Review, and Merge on the Operator Desk. Rework is not an Operator Queue; it appears as a safe stop or routed issue state. Human Review is a human decision lane, not something the Main agent can set.

Responsive behavior:

- At `1220px`, wrap top navigation into a two-row header and stack operator grids.
- At `880px`, convert topbar, section headings, route hero, lane grids, command forms, and worker metadata to single-column layouts.
- Never squeeze desktop worker cards into phone width; prioritize triage, latest evidence, and the next operator action.

## 6. Components

### App Chrome

Use `.app-chrome` with a persistent navigation band and workspace. The current UI kit uses a sticky top navbar with brand lockup, Operator Desk/Lanes/Events/Settings navigation, and compact runtime pills. Keep navigation copy concise and operational.

### Topbar

The topbar communicates the current page and readiness:

- Tiny readiness pills for Canonical checkout, Doctor, Auth, and Backend health.

### Attention Card

The most important Operator Desk component. It must include:

- Issue tag.
- State / lane type.
- Human-readable title.
- Latest evidence preview.
- Recommendation.
- One primary next action.

Use warning/danger border treatments only when operator action is truly required.

### Lane Card

Source evidence: `source_examples/web/.svelte-kit/output/server/chunks/LaneCard.js`.

Lane cards show lane name, posture, a compact View lane action, active/retrying/blocked metrics, and latest evidence. Use them for Main, Review, and Merge summaries. Keep lane cards breathable: stack lane summaries vertically inside constrained preview frames, use 16px card padding, keep metrics compact, and reserve full-size filled buttons for primary human decisions rather than secondary lane navigation.

### Worker Card

Use on lane detail pages, not the main Operator Desk. Include issue, current action, backend/session, elapsed time, latest evidence, and target transition.

### Command Console

Use for safe command generation and runtime output. It should include editable fields, write-mode controls, command preview, output status, and warning states. Commands and output use mono.

### Events

Recent events on Operator Desk are capped at 3 and low priority. Events and timeline comments are more important on Events and lane detail pages.

### Forms

Inputs are theme-aware surfaces, 48px minimum height, 4px radius, and use `--ss-focus-ring` on focus. In Daylight they should remain crisp with visible borders; in Night they use House Green input fields. Textareas can be 132px minimum. Labels are direct and action oriented.

### Drawers

Action drawers enter from the right, with a theme-aware surface, tight evidence cards, and explicit decision buttons. Do not bury destructive or state-changing actions.

## 7. Motion & Interaction

Use short, calm transitions:

- Fast: 150ms for button press/scale.
- Base: 200ms for background, border, and color changes.
- Easing: `cubic-bezier(0.25, 0.46, 0.45, 0.94)`.

Interaction patterns:

- Buttons scale to `0.95` on active press.
- Active segmented buttons use the theme accent fill and dark text.
- Focus rings are accent-tinted and visible.
- Disabled controls use `opacity: 0.48`.
- Respect `prefers-reduced-motion` by removing non-essential transitions in generated artifacts.

## 8. Voice & Brand

Voice is precise, evidence-backed, and low drama. Prefer:

- "Needs human decision"
- "Review pass recorded"
- "Merge blocked by dirty PR"
- "Canonical checkout ready"
- "Need Human Input"
- "Agent Review"

Avoid:

- "AI magic"
- "Supercharged"
- "10x"
- "Autonomous brain"
- "Sit back while agents do everything"

Copy should always reveal the next safe action and the evidence behind it. Do not invent metrics.

## 9. Anti-patterns

- Purple/violet AI gradients.
- Dense monitoring walls on the main screen.
- Tiny terminal-style text as the primary design language.
- Mascot, fantasy, or decorative imagery.
- Generic chat UI as the dominant metaphor.
- Worker internals on the Operator Desk when a lane detail page is more appropriate.
- Treating Rework as a main operator queue.
- Letting Main move work directly to Human Review.
- Hiding PR links, workpad evidence, review ledgers, or Doctor findings.
- Product artifacts that show design metadata, viewport selectors, target counts, or theme knobs.

## Source Anchors

- Product README excerpt and source inventory: `context/local-code/shea-symphony.md`.
- Live cockpit CSS tokens and components: `app/src/app.css`.
- LaneCard implementation: `source_examples/web/.svelte-kit/output/server/chunks/LaneCard.js`.
- Runtime/workflow policy: `context/local-code/shea-symphony/files/docs/bootstrap/SHEA_WORKFLOW.md`.
- Product specification: `context/local-code/shea-symphony/files/docs/bootstrap/SHEA_SYMPHONY_SPEC.md`.
- Dogfood capability map: `context/local-code/shea-symphony/files/docs/dogfood-readiness.md`.
