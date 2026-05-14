# Jade Symphony Workflows

`workflows/jade-symphony.md` is the canonical normal operator workflow for Jade
Symphony self-dogfood.

Use it for live Project #9 operations:

```bash
jade-symphony loop workflows/jade-symphony.md --write
jade-symphony forge workflows/jade-symphony.md --interactive
```

The current CLI command names are still the explicit debug/runtime names:

```bash
cargo run -- run-loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- forge-interactive --workflow workflows/jade-symphony.md
cargo run -- review-loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- merge-once workflows/jade-symphony.md --write
```

`examples/` is for fixture workflows, demos, and compatibility references. Do
not add a second normal dogfood workflow for a specific lane; lane selection
belongs in the command controller and this workflow config.
