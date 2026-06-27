# Hayate Docs

Documentation site built with [Rspress v2](https://rspress.dev). Source lives in `docs/docs/`, output goes to `docs/doc_build/`.

## Quick start

```bash
pnpm install    # first time only
pnpm dev        # live preview at http://localhost:5173
pnpm build      # production build
pnpm preview    # preview the production build
```

## Prebuild

Before every build, `scripts/copy-assets.mjs` copies install scripts and the logo from the project root into `docs/public/`. These are served as static files by the site and used by the install instructions on the home page.

## Writing docs

- MDX syntax (Markdown + JSX components)
- Frontmatter `description` required on every page for SEO and llms.txt generation
- Navigation is configured in `_nav.json` and per-section `_meta.json`

## Formatting

```bash
pnpm format
```
