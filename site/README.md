# The documentation site

This folder builds the nexc-engine documentation from the Markdown the repository already has.

## Running it

| Command           | What it does                                                                        |
| ----------------- | ----------------------------------------------------------------------------------- |
| `make docs`       | Builds the site and serves it on http://localhost:4173/nexc-engine/                 |
| `make docs-build` | Builds it to `site/.vitepress/dist` and fails on a link to a page that does not exist |
| `make docs-dev`   | Serves it with live reload on http://localhost:5174/nexc-engine/                    |
| `make openapi`    | Refreshes `public/openapi.json`, the document the API reference shows               |

The site needs Node.js 22.12 and pnpm 10; the targets run `pnpm install --frozen-lockfile` first. `DOCS_BASE` sets the path the site is served under (default `/nexc-engine/`, the GitHub Pages path of the repository; `/` for a root).

## Publishing

`.github/workflows/docs.yml` builds the site and deploys it to GitHub Pages on every push to `main` and on every `v*` tag; it takes the base path from `actions/configure-pages`, so a custom domain needs no change here. Pages must be enabled once in the repository settings (Settings, Pages, Source: GitHub Actions); until then the workflow builds and the deploy step waits for the environment. Pull requests build the site in the `docs` job of `ci.yml` without deploying, so a broken link is caught before merge.

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

Screenshots are copied from `docs/media` into `public/media` at build time, and the favicon from the app. Neither copy is committed.

## Writing for the site

The style is the one of the project's own documents, made explicit:

| Rule                                   | In practice                                                                                                                                  |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Sentence case                          | "Install in 60 seconds", not "Install In 60 Seconds". Product names keep their capitals: AgentSecOps, Docker Compose, Claude.                 |
| One idea per sentence                  | A sentence that needs a semicolon usually wants to be two. Keep most sentences under twenty-five words.                                      |
| One sentence of purpose                | Every page starts with one sentence that says what the page is for. It is the only paragraph between the title and the first heading or include. |
| Tables for options                     | Settings, commands, comparisons and choices go in a table. How something works is prose.                                                     |
| No em dashes                           | Use a comma, a colon, or a new sentence.                                                                                                     |
| Name things as the app does            | **Settings, Alerts** (bold, the path with commas), `NEXC_PUBLIC_URL`, `make demo-secops`.                                                    |
| Say it once                            | Link to the page that explains a thing rather than explaining it again. Prose that exists in a source file is included as a region.           |
| Code in backticks, commands pasteable  | Paths, variables and values in backticks. Commands in `sh` blocks that work when pasted.                                                     |

The landing page of each section (`start/index.md`, `concepts/index.md`, `secops/index.md`, `operate/index.md`, `reference/index.md`) is the model: a purpose sentence, a table of its pages with one line each on what they are for, then the included overview.
