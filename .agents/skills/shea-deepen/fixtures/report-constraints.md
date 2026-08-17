# Report Constraints

## Expected behavior

- Write `.shea/local/deepen/<run-id>/.shea-deepen-run.json` and `report.html`.
- Require the path to be Git-ignored and keep `report.html` at or below 500 KiB.
- Include at most three candidate cards and at most one top recommendation.
- Use inline HTML/CSS/SVG only, with no CDN, remote script, remote font,
  Tailwind, Mermaid, network URL, or linked asset.
- Require exact operator confirmation before marker-validated retention cleanup.
