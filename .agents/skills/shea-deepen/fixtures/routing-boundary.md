# Primary-Object Routing Boundary

## Prompts and expected routes

- `评估一下当前 docs 的状态` stops without Deepen or Doctor and routes to
  `$shea-docs` only when installed.
- `文档说的是 Temporal，但代码还是 Legacy` stops without Deepen or Doctor
  because documentation correctness and conflicting claims belong to Docs.
- `这些 Markdown prompt 的组织方式导致跨文件修改` continues through Deepen
  because the primary object is structural architecture and change locality.
- `Review prompt 让 Agent 卡住了，帮我修` routes to `$shea-doctor` because the
  primary object is a concrete stuck execution requiring repair.
- `看看代码有哪些值得深化的模块边界` continues through Deepen because the
  primary object is code architecture and module depth.

## Expected behavior

- Route by the primary object, not by `.md` or another file extension.
- Allow documentation as Deepen evidence only after an architecture scope is
  bound; allow code and tests as Docs evidence without changing Docs ownership.
- Do not use Deepen or Doctor as a fallback for documentation work when
  `$shea-docs` is unavailable.
