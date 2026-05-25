# Shea Symphony App UI Kit

This kit is a runnable applied interface example for Shea Symphony product work. It uses the package tokens in `../../colors_and_type.css`, renders in Daylight mode by default, and composes modular React components into an Operator Desk surface. Use the paired Daylight/Night preview cards under `../../preview/` when checking component changes against both themes.

## Structure

- `index.html` loads React, ReactDOM, Babel, `../../colors_and_type.css`, and every component under `components/`.
- `components/App.jsx` composes the Operator Desk, top navbar, readiness topbar, human triage cards, lane summaries, decision ledger, and recent events.
- `components/Sidebar.jsx` is kept for loader compatibility but renders the top navbar: Operator Desk, Lanes, Events, Settings, plus compact runtime status.
- `components/AssistantsList.jsx` adapts the source-backed LaneCard structure into reusable lane summary cards.
- `components/ChatArea.jsx`, `components/MessageBubble.jsx`, and `components/InputBar.jsx` provide an evidence ledger and editable operator decision input.

## Usage Workflow

1. Read `../../DESIGN.md` for product boundaries, state language, and anti-patterns.
2. Load `../../colors_and_type.css` before component CSS.
3. Start from `components/App.jsx` when making Shea Symphony cockpit screens.
4. Keep Operator Desk focused on the top 1 to 3 human decisions.
5. Put worker internals in lane detail screens, not the main desk.

## Design Notes

The kit follows the captured Svelte cockpit CSS from `context/local-code/shea-symphony/files/web/src/app.css` while applying the current review direction: primary navigation is a sticky top navbar, runtime posture sits in compact pills, the page topbar only carries readiness, and the Operator Desk keeps 8px cards, lane metrics, raised evidence panels, and responsive collapses at 1220px and 880px. Lane navigation uses compact actions so worker summaries stay scannable; reserve full-size filled buttons for primary human decisions. Daylight is the default review mode and uses Starbucks Green for brand signal, Green Accent for primary actions, and Neutral Warm/Ceramic surfaces. Set the root document to `data-theme="night"` to inspect the House Green cockpit mode.

## Source Basis

- Product overview and surfaces: `context/local-code/shea-symphony.md`.
- Live app tokens and component CSS: `context/local-code/shea-symphony/files/web/src/app.css`.
- Source LaneCard implementation: `source_examples/web/.svelte-kit/output/server/chunks/LaneCard.js`.
- Preserved runtime favicon: `../../build/favicon.svg`.
