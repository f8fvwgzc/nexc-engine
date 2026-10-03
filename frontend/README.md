# nexc-engine — frontend

React SPA for nexc-engine: log in, sketch a goal as a graph of topics/tasks on an Obsidian-style
D3 canvas, let the backend LLM refine it (streamed over SSE), and run it as a DAG with live status,
streamed output, token/cost tracking and downloadable artifacts.

The API it talks to is specified in [`../docs/CONTRACT.md`](../docs/CONTRACT.md) — the single
source of truth. Everything here is written against that contract; there is no mock mode.

## Quick start

```bash
pnpm install
cp .env.example .env.local   # optional — defaults work with the dev proxy
pnpm dev                     # http://localhost:5173, proxies /api (HTTP + WS) → http://localhost:8080
```

The backend must be running on `:8080` for anything past the landing/login pages.

## Scripts

| Script                         | What it does                                                                                    |
| ------------------------------ | ----------------------------------------------------------------------------------------------- |
| `pnpm dev`                     | Vite dev server on 5173 with `/api` proxy (`ws: true`)                                          |
| `pnpm build`                   | `pnpm typecheck` (TypeScript 7 `tsc`) then `vite build`                                         |
| `pnpm preview`                 | Serve `dist/` (port 4173, same `/api` proxy)                                                    |
| `pnpm typecheck`               | `tsc -p tsconfig.app.json && tsc -p tsconfig.node.json` (both `noEmit`)                         |
| `pnpm lint`                    | ESLint 10 flat config, type-aware typescript-eslint, react-hooks 7                              |
| `pnpm test`                    | Vitest 5 + Testing Library (jsdom)                                                              |
| `pnpm gen:api`                 | `openapi-typescript` from the running backend's `/api/openapi.json` → `src/lib/api/schema.d.ts` |
| `pnpm format` / `format:check` | Prettier (+ Tailwind class sorting)                                                             |

## Environment

Validated with zod at startup (`src/lib/env.ts`); invalid values fail fast.

| Var                 | Default     | Notes                                                     |
| ------------------- | ----------- | --------------------------------------------------------- |
| `VITE_API_BASE_URL` | `/api/v1`   | Path or absolute URL. WebSocket URLs are derived from it. |
| `VITE_API_DOCS_URL` | `/api/docs` | Target of the "API docs" sidebar link (Scalar UI).        |

## Structure

```
src/
  main.tsx                 entry (StrictMode + <App/>)
  app/                     App.tsx, router.tsx (createBrowserRouter, lazy routes), route-modules.ts,
                           providers/ (theme, query, helmet, auth bootstrap)
  components/
    ui/                    shadcn/ui (generated — `shadcn add`, do not hand-edit beyond theming)
    custom-ui/             product design layer built on shadcn (see below)
    layout/                AppShell, AppSidebar, AppHeader, PublicLayout, PrivateRoute, PublicRoute, …
    seo/                   <Seo title description />
  features/<name>/         auth, graphs (dashboard/templates), graph (canvas, panels, toolbar, hooks),
                           runs, agents, memory, settings, command (⌘K palette), landing
                           each with api.ts (+ query options), components/, hooks/
  lib/
    api/                   fetch client (auth + single-flight refresh), problem+json errors,
                           form-error mapping, downloads, schema.d.ts (generated), contract-drift.ts
    realtime/              sse.ts, ws.ts, ticket, backoff, keyed throttle, SSE frame parser
    env.ts query-keys.ts query-client.ts format.ts utils.ts
  schemas/                 zod mirrors of CONTRACT §4/§6/§7 — runtime guards + source of TS types
  stores/                  zustand: auth (in-memory token), graph canvas (sliced), command palette
  hooks/                   generic hooks (theme, reduced motion, debounce, dismissible, mobile)
  pages/                   thin route components (one default export each, lazy-loaded)
  styles/                  Tailwind v4 CSS-first theme (index.css), canvas.css, landing.css
  test/                    setup + contract fixtures
```

## Conventions

- **Server state → React Query, UI/live state → zustand.** Query keys come from
  `lib/query-keys.ts`. Pages use `useSuspenseQuery` inside a page-level `<Suspense>` with a skeleton;
  every route has an `errorElement`.
- **Every response is validated** with the zod schemas in `src/schemas/` (dev and prod). A mismatch
  throws `ContractError`; HTTP errors are RFC 7807 `ApiError`s whose `errors` map onto form fields
  (`applyProblemToForm`) or surface as toasts (global `MutationCache` handler; opt out with
  `meta: { errorToast: false }`).
- **Generated OpenAPI types are a drift check, not a dependency.** `schema.d.ts` is a placeholder
  until `pnpm gen:api` runs against the backend; `lib/api/contract-drift.ts` then fails
  `pnpm typecheck` if a generated schema stops matching our zod types.
- **Auth:** the access token lives only in memory (zustand). On boot we `POST /auth/refresh`
  (cookie + `X-Requested-With: nexc`); the client retries once after a single-flight refresh on 401
  and refreshes proactively before expiry. Losing the session clears the query cache.
- **Realtime:** single-use tickets (`POST /realtime/tickets`) authenticate each SSE/WS connection,
  so reconnects always fetch a fresh ticket (exponential backoff with jitter). Streamed output and
  logs are coalesced (60 ms) before touching the store.
- **Canvas:** React renders every SVG element (memoized); `CanvasEngine` (D3) owns force
  simulation, zoom and drag and writes `transform`/`d` attributes directly per tick — no React
  render per frame. d3 and the canvas live in their own lazy chunk.
- **Head tags:** exactly one `<Seo>` per page. With React 19, react-helmet-async renders native
  hoisted `<title>`/`<meta>` without de-duplication, so never nest a second one.
- **Security:** no tokens in storage, no `dangerouslySetInnerHTML` (lint-enforced), user content is
  rendered as text (`whitespace-pre-wrap`), external links use `rel="noopener noreferrer"`, a
  strict CSP meta tag is injected into production builds (the theme no-flash script is the static
  `public/theme-init.js`, so `script-src 'self'` suffices).
- Motion is CSS-only (Tailwind keyframes in `styles/index.css`) and disabled under
  `prefers-reduced-motion`.

## The custom-ui layer

`components/ui` is shadcn's generated, unopinionated primitives. `components/custom-ui` is the
product's own vocabulary composed from them — feature code should reach for these first:

| Component                                                                                    | Built on                               | Purpose                                                                              |
| -------------------------------------------------------------------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------------ |
| `GlassCard`                                                                                  | div + tokens                           | translucent surface, optional interactive lift                                       |
| `AnimatedButton`                                                                             | `Button`, `Spinner`                    | loading state, press feedback, brand glow                                            |
| `StatusBadge` / `status-meta`                                                                | tokens, lucide                         | node/run status colors; `running` pulses; `cached` derived from `succeeded + cached` |
| `FormField`                                                                                  | `Field*`, react-hook-form `Controller` | label/description/error + aria wiring                                                |
| `OptionSelect`                                                                               | `Select`                               | typed fixed-option select                                                            |
| `ConfirmDialog`                                                                              | `AlertDialog`                          | destructive confirmations                                                            |
| `EmptyState`                                                                                 | `Empty`                                | icon + copy + action                                                                 |
| `PageHeader`, `StatTile`, `GradientText`, `KbdHint`, `CopyButton`, `Spinner`, `NodeKindIcon` | various                                | page chrome & small affordances                                                      |
| `FadeIn`, `Stagger`                                                                          | CSS keyframes                          | entrance motion                                                                      |

To restyle the whole app, change tokens in `styles/index.css` (`:root` / `.dark`), not components.

## Contract notes (ambiguities resolved here)

- **`cached` status.** `NodeStatus` has no `cached`; the UI shows "Cached" for
  `succeeded` + `cached: true` (from `node.status` events / `NodeRun.cached`).
- **`node.tokens`** is treated as the node's cumulative usage for the current attempt (we keep
  the max seen), and run totals are the sum across nodes until `run.finished` delivers the
  authoritative `Run`.
- **Template ids** are slugs (`research-report-docx`), so `GraphTemplate.id` is not validated as a
  UUID; all other ids are.
- **Timestamps** accept any RFC 3339 offset (`Z` or `+00:00`) and fractional seconds.
- **Agent `budget_tokens: 0`** is displayed as "no limit".
- **Plan discard** has no endpoint; discarding only clears the local ghost proposal.
- **Node positions** are persisted via WS `node.move` while connected; if the socket is down the
  final drop falls back to `PATCH /graphs/{gid}/nodes/{nid}`.
- **`/app/runs`** aggregates `GET /graphs/{gid}/runs` across graphs (there is no global runs list).
- **SSE reconnect** cannot resume via `Last-Event-ID` with a new ticket, so after reconnecting we
  refetch the graph/runs and, if a plan was streaming, `GET /graphs/{gid}/plans/{pid}`.

## Toolchain note: TypeScript 7

`typescript@7` (the native Go compiler) is the project compiler — `pnpm typecheck`/`build` run its
`tsc`. TS 7 ships no JavaScript compiler API, but `typescript-eslint` and `openapi-typescript`
still `require("typescript")`. `.pnpmfile.cjs` therefore gives exactly those packages a private
`typescript@5.9.3` dependency (instead of the peer), so type-aware linting and `gen:api` keep
working while everything else uses TS 7.
