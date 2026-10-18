# The documentation site

This folder builds the nexc-engine documentation from the Markdown the repository already has.

## Running it

The site needs Node.js 22.12 and pnpm 10.

```sh
cd site
pnpm install --frozen-lockfile
pnpm build      # to .vitepress/dist; fails on a link to a page that does not exist
pnpm preview    # serves the build on http://localhost:4173/nexc-engine/
pnpm dev        # live reload while writing
```

`DOCS_BASE` sets the path the site is served under (default `/nexc-engine/`, the GitHub Pages path of the repository; `/` for a root). The API reference shows `public/openapi.json`, an export of `nexc openapi`; refresh it when the API changes.

## How it is built

The generator is [VitePress](https://vitepress.dev). It was chosen over Astro Starlight because the four things the site needs most are built in: including another Markdown file by reference (`<!--@include: ../../docs/CONTRACT.md#env-->`), failing the build on a dead link, full-text search and a dark theme. The code of our own is three small files:

| File                                | What it does                                                                                                                                              |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `.vitepress/repo-links.ts`          | Maps the repository-relative links inside the included Markdown (`CONTRACT.md`, `docs/ARCHITECTURE.md#scaling-and-state`) to the page that renders the file, or to GitHub |
| `.vitepress/mermaid.ts`             | Renders a `mermaid` fence with `theme/Mermaid.vue`, in the reader's theme                                                                                |
| `.vitepress/theme/ApiReference.vue` | Shows `public/openapi.json` with Scalar                                                                                                                   |

### Sources of truth

The prose lives in the repository, not here. A page under `site/` is a title, one sentence, and includes:

| Source                                           | Pages                                                         |
| ------------------------------------------------ | ------------------------------------------------------------- |
| `README.md`                                      | Start, Concepts overview, Operate pages, CLI, Troubleshooting |
| `docs/ARCHITECTURE.md`                           | Concepts                                                      |
| `docs/AGENTSECOPS.md`                            | AgentSecOps                                                   |
| `docs/INFRASTRUCTURE.md`                         | Infrastructure, Workspace transfers, Ask the audit            |
| `docs/CONTRACT.md`                               | Reference                                                     |
| `deploy/k8s/README.md`, `values.yaml`            | Kubernetes, Helm chart                                        |
| `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md` | Contributing, Security, Changelog                             |

A source is cut into regions with marker comments, invisible on GitHub:

```md
<!-- #region env -->
| Var | Default | Used by |
...
<!-- #endregion env -->
```

Inside a code block the marker is a line comment, `// #region alerts`. To put a new part of a source on the site, add a pair of markers around it and include the region from a page. To add a new source file, add its path to `ROUTES` in `.vitepress/repo-links.ts` so links to it resolve, and its page to the sidebar in `.vitepress/config.ts`.

Screenshots are copied from `docs/media` into `public/media` at build time, and the favicon from the app; neither copy is committed.
