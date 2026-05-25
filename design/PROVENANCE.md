# Provenance Notes

## Intake Commands

The project source context required bounded repository and local-code intake before final design-system authoring.

GitHub evidence was collected with:

```sh
"$OD_NODE_BIN" "$OD_BIN" tools connectors github-design-context --repo 'https://github.com/Alive24/shea-symphony' --output context/github/Alive24-shea-symphony.md
```

Result: `Read method: git-clone`, with snapshots under `context/github/Alive24-shea-symphony/files/`.

Local code evidence was collected with:

```sh
"$OD_NODE_BIN" "$OD_BIN" tools connectors local-design-context --path '/Volumes/Bohemialive/GitHub/shea-symphony' --output context/local-code/shea-symphony.md
```

Result: `Read method: local-folder`, with snapshots under `context/local-code/shea-symphony/files/`.

## Preserved Assets

- `build/favicon.svg` was copied byte-for-byte from `context/local-code/shea-symphony/files/web/build/favicon.svg`.
- `assets/favicon.svg` is a convenience alias of the same source asset.

## Source Examples

- `source_examples/web/.svelte-kit/output/server/chunks/LaneCard.js` preserves the captured compiled LaneCard implementation.
- `source_examples/web/.svelte-kit/output/server/entries/pages/_layout.js` preserves the captured SvelteKit layout entry.

## Design Decisions

The generated system prefers the live cockpit CSS in `web/src/app.css` over the earlier setup note's green/cream starting language because it is concrete implementation evidence. The preserved favicon still carries the green/cream runtime asset lineage, and the token file exposes those tones as supporting asset colors rather than primary app chrome.
