# AGENTS — Hayate Docs

## Setup

```bash
cd docs && pnpm install
```

## Development

```bash
pnpm dev       # Live preview at localhost:5173
pnpm build     # Production build into doc_build/
pnpm format    # Format with Prettier
```

## File structure

- `docs/docs/` — source MDX pages
- `docs/public/` — static assets, auto-populated by prebuild
- `docs/styles/custom.css` — global CSS overrides
- `docs/rspress.config.ts` — site configuration
- `doc_build/` — build output (gitignored)

## Conventions

- Every `.mdx` page must have a `description` frontmatter field
- Use `:::\` fenced containers for tips/warnings
- Navigation updates go in `_nav.json` and `_meta.json`
