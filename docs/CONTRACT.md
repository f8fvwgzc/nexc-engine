# nexc-engine — system contract

This file is the single source of truth that the **backend** (`backend/`, Rust axum), the
**frontend** (`frontend/`, React) and the **agent runtime** (`agent-runtime/`, Python) agree on.
Change the contract and every implementation together. The backend also publishes the machine
readable form at `GET /api/openapi.json` (utoipa); the frontend generates TypeScript types from it
(`npm run gen:api`) and validates responses at runtime with zod.

## 1. Topology & ports

| Service                  | Dev port | Purpose                                                                                                  |
| ------------------------ | -------: | -------------------------------------------------------------------------------------------------------- |
| backend (`nexc serve`)   |     8080 | REST + SSE + WebSocket API, graph engine, scheduler, memory, orchestrator                                |
| PostgreSQL 17            |     5432 | the only database (no SQLite anywhere); local dev runs it via `make db-up` (docker `postgres:17-alpine`) |
| frontend (vite dev)      |     5173 | SPA; dev server proxies `/api` (HTTP + WS) to 8080                                                       |
| agent-runtime            |     8090 | Python agent runtime, HTTP (internal only, never exposed publicly)                                       |
| agent-runtime gRPC       |     8095 | the runtime's `AgentRuntime/Execute` stream (§8), the wire the backend speaks; internal only              |
| backend gRPC (optional)  |    50052 | the Agent Firewall API (§8) on `NEXC_GRPC_ADDR`; off by default, service to service only, never a browser |
| OTLP collector (optional)|     4317 | where `NEXC_OTLP_ENDPOINT` points: the trace spine exported as OpenTelemetry spans (§8)                   |
| texc-symphony (optional) |     4000 | external coding-task executor                                                                            |
| onto-intelli (optional)  |    50051 | gRPC ontology and audit engine: the trace's facts go there, "ask the audit" answers from them (§5, audit) |

In Docker/K8s the frontend is served by unprivileged nginx on 8080 which proxies `/api` → backend:8080.

## 2. Environment variables (one `.env` at repo root; see `.env.example`)

| Var                                                                     | Default                                                   | Used by                                                                                                                                                                                                                                                                                  |
| ----------------------------------------------------------------------- | --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `NEXC_ENV`                                                              | `development`                                             | backend (`production` forces secure cookies, HSTS, refuses weak secrets)                                                                                                                                                                                                                 |
| `NEXC_HOST` / `NEXC_PORT`                                               | `0.0.0.0` / `8080`                                        | backend                                                                                                                                                                                                                                                                                  |
| `NEXC_DATABASE_URL`                                                     | `postgres://nexc:<POSTGRES_PASSWORD>@localhost:5432/nexc` | backend (PostgreSQL 17; `sslmode=require` recommended outside dev)                                                                                                                                                                                                                       |
| `NEXC_DB_PORT`                                                          | `5432`                                                    | host port for the local dev/compose postgres (`make init` builds `NEXC_DATABASE_URL` from it)                                                                                                                                                                                            |
| `NEXC_DB_MAX_CONNECTIONS`                                               | `20`                                                      | backend pool size                                                                                                                                                                                                                                                                        |
| `POSTGRES_USER` / `POSTGRES_PASSWORD` / `POSTGRES_DB`                   | `nexc` / — (generated by `make init`) / `nexc`            | postgres container (compose/k8s)                                                                                                                                                                                                                                                         |
| `NEXC_DATA_DIR`                                                         | `./data`                                                  | backend (artifacts at `<data>/artifacts/<run_id>/...`)                                                                                                                                                                                                                                   |
| `NEXC_JWT_SECRET`                                                       | — (required, ≥32 bytes)                                   | backend                                                                                                                                                                                                                                                                                  |
| `NEXC_MASTER_KEY`                                                       | — (required, base64 of 32 bytes)                          | backend: AES-256-GCM for stored LLM API keys                                                                                                                                                                                                                                             |
| `NEXC_CORS_ORIGINS`                                                     | `http://localhost:5173`                                   | backend (comma separated)                                                                                                                                                                                                                                                                |
| `NEXC_COOKIE_SECURE`                                                    | `false` dev / `true` prod                                 | backend                                                                                                                                                                                                                                                                                  |
| `NEXC_ALLOW_SIGNUP`                                                     | `true`                                                    | backend                                                                                                                                                                                                                                                                                  |
| `NEXC_ADMIN_EMAIL` / `NEXC_ADMIN_PASSWORD`                              | —                                                         | backend bootstraps a platform administrator on first start if both set                                                                                                                                                                                                                   |
| `NEXC_ACCESS_TTL_SECS` / `NEXC_REFRESH_TTL_SECS`                        | `900` / `1209600`                                         | backend                                                                                                                                                                                                                                                                                  |
| `NEXC_LLM_PROVIDER`                                                     | `anthropic`                                               | backend (`anthropic` \| `openai_compatible` \| `claude_code` \| `demo`). `claude_code` = local Claude Code CLI (`claude -p`) with its own login, no API key. `demo` = deterministic offline provider so anyone can try plan + run with NO API key (outputs are clearly labelled as demo) |
| `NEXC_LLM_MODEL`                                                        | `claude-opus-5`                                           | backend + runtime                                                                                                                                                                                                                                                                        |
| `NEXC_LLM_BASE_URL`                                                     | provider default                                          | backend                                                                                                                                                                                                                                                                                  |
| `ANTHROPIC_API_KEY`                                                     | —                                                         | server-wide default key (a user's own stored key wins)                                                                                                                                                                                                                                   |
| `NEXC_CLAUDE_BIN` / `RUNTIME_CLAUDE_BIN`                                | `claude`                                                  | Claude Code CLI for the `claude_code` provider (backend / runtime)                                                                                                                                                                                                                       |
| `NEXC_RUNTIME_URL`                                                      | `http://localhost:8090`                                   | backend → runtime                                                                                                                                                                                                                                                                        |
| `NEXC_RUNTIME_TOKEN`                                                    | — (required, ≥32 chars)                                   | backend + runtime shared bearer secret                                                                                                                                                                                                                                                   |
| `NEXC_AGENT_TOKEN_KEY`                                                  | — (derived from `NEXC_RUNTIME_TOKEN` when unset)          | backend + runtime: signs and verifies the per-run agent tokens (§8); ≥32 chars when set                                                                                                                                                                                                  |
| `NEXC_RUNTIME_PROTOCOL`                                                 | `grpc`                                                    | backend: the wire to the runtime, `grpc` (one `AgentRuntime/Execute` stream per node attempt, §8) or `ndjson` (`POST /v1/execute`, kept for one release)                                                                                                                                  |
| `NEXC_RUNTIME_GRPC_URL`                                                 | `http://localhost:8095`                                   | backend → the runtime's gRPC endpoint                                                                                                                                                                                                                                                    |
| `RUNTIME_GRPC_PORT`                                                     | `8095`                                                    | runtime: the gRPC stream next to its HTTP port; `0` turns it off                                                                                                                                                                                                                         |
| `NEXC_GATEWAY_CALLS_PER_MINUTE` / `NEXC_GATEWAY_TIMEOUT_SECS` / `NEXC_GATEWAY_MAX_RESPONSE_BYTES` | `120` / `30` / `1048576`               | backend: the tools gateway (§8): calls one agent may make per minute (burst a quarter of it), the longest one call may take, the largest response passed on                                                                                                                               |
| `NEXC_GRPC_ADDR`                                                        | — (off)                                                   | backend: where the Agent Firewall API listens (`0.0.0.0:50052`), with reflection and `grpc.health.v1`; unset keeps it off                                                                                                                                                                |
| `NEXC_FIREWALL_SIGNING_KEY`                                             | — (derived from `NEXC_MASTER_KEY` when unset)             | backend: base64 of 32 bytes, the Ed25519 seed the firewall signs decisions with                                                                                                                                                                                                          |
| `NEXC_OTLP_ENDPOINT`                                                    | — (off)                                                   | backend: OTLP/gRPC endpoint (`http://host:4317`) the trace spine is exported to; unset keeps it off                                                                                                                                                                                       |
| `NEXC_PUBLIC_URL`                                                       | `http://localhost:8080`                                   | backend: where the console is reached from outside; the links every alert delivery carries (`/app/secops`, `/app/runs/{id}?span=`, `/app/agents?agent=`)                                                                                                                                |
| `NEXC_ALERT_ALLOW_PRIVATE_HOSTS`                                        | `false`                                                   | backend: lets alert destinations name private and loopback hosts (development only: a receiver on this machine); otherwise such a URL is refused (422), as the gateway refuses the host                                                                                                   |
| `NEXC_DB_IMAGE`                                                         | `postgres:17-alpine`                                      | `scripts/db.sh`: image of the dev database. `pgvector/pgvector:pg17` enables HNSW memory search; only for a new volume                                                                                                                                                                   |
| `NEXC_SYMPHONY_ENABLED`                                                 | `false`                                                   | backend                                                                                                                                                                                                                                                                                  |
| `NEXC_SYMPHONY_URL`                                                     | `http://localhost:4000`                                   | backend                                                                                                                                                                                                                                                                                  |
| `NEXC_SYMPHONY_WORKFLOW`                                                | `./data/symphony/WORKFLOW.md`                             | backend writes the managed memory-tracker workflow here                                                                                                                                                                                                                                  |
| `NEXC_SYMPHONY_AGENT_COMMAND`                                           | — (Symphony default: Codex)                               | backend: `codex.command` of the managed workflow, e.g. the absolute path of `scripts/symphony-claude-agent.py` to use the Claude Code CLI                                                                                                                                                |
| `NEXC_ONTO_URL`                                                         | — (off)                                                   | backend: gRPC endpoint of onto-intelli (`http://host:50051`); unset turns "ask the audit" off (no export, the ask routes answer 503)                                                                                                                                                      |
| `NEXC_ONTO_TOKEN`                                                       | — (generated by `make init`)                              | backend: bearer token onto-intelli knows the backend by (`[[auth.tokens]]` in its onto-intelli.toml; unset when its auth is off)                                                                                                                                                             |
| `NEXC_ONTO_WORKSPACE_PREFIX`                                            | `nexc-`                                                   | backend: each nexc workspace's facts live in the onto-intelli workspace `{prefix}{workspace id}` (≤32 of `[A-Za-z0-9._-]`)                                                                                                                                                                |
| `NEXC_MAX_CONCURRENCY` / `NEXC_MAX_ATTEMPTS` / `NEXC_NODE_TIMEOUT_SECS` | `4` / `3` / `600`                                         | backend scheduler                                                                                                                                                                                                                                                                        |
| `RUNTIME_HOST` / `RUNTIME_PORT`                                         | `0.0.0.0` / `8090`                                        | runtime                                                                                                                                                                                                                                                                                  |
| `RUNTIME_WORKSPACE`                                                     | `/tmp/nexc-runtime`                                       | runtime (per-run scratch dirs)                                                                                                                                                                                                                                                           |
| `RUNTIME_ALLOW_CODE_EXEC`                                               | `false`                                                   | runtime: enables the sandboxed `run_python` tool                                                                                                                                                                                                                                         |
| `VITE_API_BASE_URL`                                                     | `/api/v1`                                                 | frontend                                                                                                                                                                                                                                                                                 |

## 3. Conventions

- Base path `/api/v1`. JSON bodies, `snake_case` fields, ids are UUID v7 strings, timestamps RFC 3339 UTC.
- Auth: `Authorization: Bearer <access_token>` (JWT HS256, 15 min). Refresh token lives only in the
  `nexc_refresh` cookie (`HttpOnly; SameSite=Strict; Path=/api/v1/auth; Secure` when enabled) and is
  rotated on every refresh (reuse of an old token revokes the whole family).
  Cookie-authenticated endpoints (`/auth/refresh`, `/auth/logout`) also require header
  `X-Requested-With: nexc` (CSRF guard).
- Two sides. An account whose `role` is `admin` is a platform administrator: its token is accepted on
  `/auth/*` and `/admin/*` only, and gets 403 on every other route, whatever workspace memberships
  the account has. Every other account gets 403 on `/admin/*`. Access tokens carry the account's
  session epoch; suspending an account or changing its platform role raises it, which ends the
  tokens issued before (401) without waiting for them to expire.
- Errors are RFC 7807 `application/problem+json`:
  `{ "type": "about:blank", "title": "Validation failed", "status": 422, "detail": "...", "errors": { "email": ["invalid email"] } }`.
  Codes: 400 bad input, 401 unauthenticated, 403 forbidden, 404, 409 conflict (e.g. edge would create a cycle),
  413 too large, 422 validation, 429 rate limited (with `Retry-After`), 500.
- `GET /graphs/{id}` returns `ETag`; supports `If-None-Match` → 304.
- Limits: request body 1 MiB (node content max 64 KiB, title max 200 chars, ≤ 500 nodes per graph).

## 4. Types (TypeScript notation)

```ts
type Role = "admin" | "user";
interface User {
  id: string;
  email: string;
  name: string;
  role: Role;
  created_at: string;
}
interface AuthResponse {
  user: User;
  access_token: string;
  expires_in: number;
}

type LlmProvider = "anthropic" | "openai_compatible" | "claude_code" | "demo";
interface LlmSettings {
  provider: LlmProvider;
  model: string;
  base_url: string | null;
  has_api_key: boolean;
  key_hint: string | null /* e.g. "…a1b2" */;
  source:
    "user" | "workspace" | "server" | "none" /* where the key comes from */;
  scope: "user" | "workspace" | "server"; /* whose configuration is in effect */
}
// Work a user does in a workspace runs on the account they connected themselves, else on the
// workspace's credential, else on the server default; usage is spent on whichever supplied the key.
interface ModelInfo {
  id: string;
  name: string;
  released_at: string | null;
  recent: boolean; /* ≤ 120 days old */
}
interface ModelCatalog {
  provider: LlmProvider;
  models: ModelInfo[] /* newest first */;
  note: string | null;
}

// Node and relation types are data: every graph owns an ontology and `kind` fields are keys into it
// (`[a-z][a-z0-9_]{0,39}`). A new graph starts from a starter ontology (topic, research, task, code,
// document, output; depends_on, relates_to) that users and the planner may extend or rewrite.
interface NodeType {
  key: string;
  label: string;
  description: string;
  color: string /* #rrggbb */;
  icon: string;
  default_role: string;
  default_executor: Executor;
  stage: number /* position in the flow of work; suggestions point from lower to higher */;
  produces_artifact: boolean /* its nodes deliver a file */;
  allow_code_exec: boolean;
}
interface RelationType {
  key: string;
  label: string;
  description: string;
  blocking: boolean; /* source must finish before target runs; blocking relations form the DAG */
}
interface Ontology {
  node_types: NodeType[];
  relation_types: RelationType[];
} // each 1..40 types

type NodeStatus =
  | "idle"
  | "queued"
  | "running"
  | "waiting_approval" // a tool call of the node waits for a human; the run stays running
  | "succeeded"
  | "failed"
  | "skipped"
  | "cancelled";
type Executor = "llm" | "agent" | "symphony";
interface GraphNode {
  id: string;
  graph_id: string;
  title: string;
  content: string;
  kind: string /* NodeType.key */;
  tags: string[];
  x: number;
  y: number;
  status: NodeStatus;
  agent_role: string | null;
  executor: Executor;
  output: string | null;
  origin: "user" | "plan";
  created_at: string;
  updated_at: string;
}

interface GraphEdge {
  id: string;
  graph_id: string;
  source: string;
  target: string;
  kind: string /* RelationType.key */;
  blocking: boolean /* copied from the relation type */;
  reason: string /* why the two nodes are related this way, ≤ 500 chars */;
  origin: "user" | "auto" | "plan";
  weight: number;
}

interface GraphSummary {
  id: string;
  workspace_id: string | null;
  team_id: string | null;
  name: string;
  description: string;
  node_count: number;
  edge_count: number;
  updated_at: string;
}
// A graph belongs to a workspace and optionally to one of its teams. Team graphs are open to the
// team's members; workspace graphs to every member but guests. Everyone else gets 404.
interface Graph {
  id: string;
  workspace_id: string | null;
  team_id: string | null /* null: whole workspace */;
  name: string;
  description: string;
  goal: string;
  version: number;
  ontology: Ontology;
  nodes: GraphNode[];
  edges: GraphEdge[];
  created_at: string;
  updated_at: string;
}

interface EdgeSuggestion {
  source: string;
  target: string;
  score: number;
  reason: string;
}
interface GraphAnalysis {
  topo_order: string[];
  levels: string[][];
  critical_path: string[];
  cycles: string[][];
  components: string[][];
}

type PlanStatus = "streaming" | "ready" | "failed" | "applied";
interface ProposedNode {
  ref: string;
  existing_id: string | null;
  title: string;
  content: string;
  kind: string;
  agent_role: string;
  executor: Executor;
  tags: string[];
}
interface ProposedEdge {
  source_ref: string;
  target_ref: string;
  kind: string;
  reason: string;
}
interface Plan {
  id: string;
  graph_id: string;
  status: PlanStatus;
  summary: string;
  ontology: Ontology /* only the types the plan adds; merged into the graph on apply */;
  nodes: ProposedNode[];
  edges: ProposedEdge[];
  error: string | null;
  created_at: string;
}

// Organisations. Every account belongs to at least one workspace (a personal one is created on
// sign-up unless the address was invited to an existing workspace).
type WorkspaceRole = "owner" | "admin" | "member" | "guest";
type TeamRole = "owner" | "member";
interface Workspace {
  id: string;
  name: string;
  slug: string;
  role: WorkspaceRole /* the caller's */;
  member_count: number;
  created_at: string;
}
interface WorkspaceMember {
  user_id: string;
  name: string;
  email: string;
  role: WorkspaceRole;
  suspended: boolean; // by a platform administrator: cannot sign in until it is lifted
  joined_at: string;
}
interface WorkspaceInvite {
  id: string;
  email: string;
  role: WorkspaceRole;
  created_at: string;
}
interface Team {
  id: string;
  workspace_id: string;
  name: string;
  key: string /* e.g. "ENG" */;
  description: string;
  private: boolean;
  role: TeamRole | null /* the caller's */;
  member_count: number;
  created_at: string;
}
interface TeamMember {
  user_id: string;
  name: string;
  email: string;
  role: TeamRole;
  joined_at: string;
}

// Issues (Linear's model). A team owns its workflow: states are rows with a name, a colour and a
// category; only the category is a fixed vocabulary. An issue's identifier is the team key and a
// number that counts up per team.
type StateCategory =
  "backlog" | "unstarted" | "started" | "completed" | "canceled";
interface IssueState {
  id: string;
  team_id: string;
  name: string;
  category: StateCategory;
  color: string;
  position: number;
}
interface Issue {
  id: string;
  workspace_id: string;
  team_id: string;
  identifier: string /* "ENG-12" */;
  number: number;
  title: string;
  description: string;
  state: IssueState;
  priority: 0 | 1 | 2 | 3 | 4 /* none, urgent, high, medium, low */;
  assignee: { user_id: string; name: string } | null;
  agent_id: string | null;
  project_id: string | null;
  graph_id: string | null /* the graph that plans and executes the issue */;
  creator_id: string | null;
  due_date: string | null; /* YYYY-MM-DD */
  created_at: string;
  updated_at: string;
  completed_at: string | null; /* follows the state's category */
}
type ProjectStatus =
  "planned" | "started" | "paused" | "completed" | "canceled";
interface Project {
  id: string;
  workspace_id: string;
  name: string;
  description: string;
  status: ProjectStatus;
  lead_id: string | null;
  target_date: string | null;
  issue_count: number;
  closed_count: number /* of the
  issues the caller can see */;
  created_at: string;
}

// Guardrails: checked before a plan, a run or an assistant reply starts (403 with the reason).
interface Guardrails {
  monthly_token_budget:
    number | null /* tokens per UTC month, workspace-wide */;
  member_monthly_token_budget: number | null;
  allowed_providers: LlmProvider[] /* empty: all */;
  allow_code_exec: boolean;
  redact_secrets: boolean; /* scrub keys/tokens from context sent to models */
}
interface AssistantReply {
  reply: string;
  created: Issue[];
  skipped: string[];
  memories_used: number;
}

// Token usage. Every LLM call that spent tokens is recorded with who caused it and whose account paid.
interface UsageTotals {
  calls: number;
  tokens_in: number;
  tokens_out: number;
  cost_usd: number;
  context_chars_saved: number; /* upstream characters left out of prompts */
}
type UsageSlice<K> = { key: K } & UsageTotals;
interface UsageReport {
  scope: "workspace" | "own";
  days: number;
  totals: UsageTotals;
  by_day: UsageSlice<string /* UTC date */>[];
  by_member: UsageSlice<{ user_id: string | null; name: string }>[];
  by_model: UsageSlice<{ provider: string; model: string }>[];
  by_purpose: UsageSlice<
    "plan" | "node" | "memory" | "assistant" | "embedding" | "summary"
  >[];
  by_credential: UsageSlice<
    "user" | "workspace" | "server"
  >[]; /* whose account paid */
}

interface GraphTemplate {
  id: string;
  name: string;
  description: string;
  category: string;
  node_count: number;
  tags: string[];
}
// built-ins (at least): research-report-docx, rest-api-service, market-analysis, blog-series, data-pipeline, product-launch-plan

type RunStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled";
interface NodeRun {
  node_id: string;
  status: NodeStatus;
  attempt: number;
  executor: Executor;
  tokens_in: number;
  tokens_out: number;
  cached: boolean;
  error: string | null;
  started_at: string | null;
  finished_at: string | null;
  output_preview: string | null;
  policy_versions: PolicyVersions; // the policy versions that governed the node, by level
  context_labels: DataLabel[]; // the classifications of the data the node was given (its context and its upstream nodes')
}
interface Run {
  id: string;
  graph_id: string;
  status: RunStatus;
  tokens_in: number;
  tokens_out: number;
  cost_usd: number;
  started_at: string | null;
  finished_at: string | null;
  created_at: string;
  trace_hash: string | null; // hex SHA-256 sealing the trace once the run finished; null while it runs or when nothing was traced
  policy_mismatches: number; // actions the runtime reported that the run's policy did not allow (each a `policy` span and a `policy_mismatch` audit entry)
  policy_versions: PolicyVersions; // the workspace policy version that governed the run (`team` and `agent` are null here; each node run has every level)
  node_runs: NodeRun[];
}
interface Artifact {
  id: string;
  run_id: string;
  node_id: string;
  path: string;
  size: number;
  mime: string;
  created_at: string;
  quarantined_at: string | null; // the DLP classifier kept the file in; downloadable only once released
  quarantine_reason: string; // `dlp.labels: content derived from restricted data may not leave as an artifact`, `dlp.secrets: 1 credential in the content`, ...
  released_at: string | null;
}
type AlertKind = "dlp" | "risk" | "injection" | "policy";
interface Alert {
  id: string;
  workspace_id: string;
  agent_id: string | null;
  principal: string;
  run_id: string | null;
  node_id: string | null;
  graph_id: string | null;
  span_id: string | null; // the policy span that recorded the finding
  kind: AlertKind;
  severity: Severity;
  rule: string; // `dlp.secrets`, `dlp.pii`, `dlp.labels`, `egress.deny`, `alerts.risk_score`, `injection`, ...
  title: string;
  detail: string;
  created_at: string;
  acknowledged_at: string | null;
  acknowledged_by: string | null;
  deliveries: number; // alert destinations it was delivered to (alert routing); 0 until the worker sent it
}

// The trace spine: one span per action of an agent, append-only, hash-chained per run in order of
// receipt by the backend: hash = SHA-256(prev_hash || canonical JSON of the span), 32 zero bytes
// before the first. The canonical JSON has every field below but the two hashes, keys sorted, no
// whitespace, times with microseconds (`2026-10-17T12:00:00.123456Z`).
type SpanKind = "llm_call" | "tool_call" | "spawn" | "data_read" | "egress" | "policy" | "approval";
type SpanStatus = "ok" | "error";
interface Span {
  id: string; // UUID v7
  run_id: string;
  node_id: string;
  agent_id: string | null; // the workspace agent assigned to the node
  principal: string; // who acted: the agent's principal (`agent://{workspace}/{name}`, sub-agents `…/{role}-{n}`), "system" for a node without an agent
  parent_id: string | null; // the turn of a tool call, the spawn of a sub-agent's turns
  kind: SpanKind;
  name: string; // the model, the tool, the child agent, or "context"
  started_at: string;
  ended_at: string | null;
  status: SpanStatus;
  attributes: Record<string, unknown>; // digests, sizes, counts and ids; never arguments or secrets
  seq: number; // position in the run's chain
  prev_hash: string; // hex
  hash: string; // hex
}
// data_read (backend, before a node runs): attributes {document_chunk_ids, memory_ids, upstream_node_ids, documents, memories, chars_saved}
// llm_call (built-in executor): attributes {provider, model, tokens_in, tokens_out, tokens_cached, stop_reason, duration_ns}
// policy (backend): attributes {decision: allow|deny|mismatch, rule, reason, action, policy_version, …}: the model allow-list before dispatch (name: the model), what the context builder withheld (name "context": withheld_document_chunk_ids, withheld_memory_ids, withheld_by_labels|topics|namespaces), a quarantine (rule "quarantine"), and every re-check disagreement (name "policy_mismatch": span_id, principal)
// llm_call, tool_call, spawn, policy (agent runtime): see §8
interface TracePage {
  spans: Span[];
  next: string | null;
}
interface TraceVerification {
  ok: boolean;
  spans: number;
  hash: string | null; // the chain's last hash
  sealed_hash: string | null; // the run's trace_hash
  broken: { span_id: string; seq: number; reason: string } | null; // the first span that does not fit
}

// Ask the audit (AgentSecOps phase 3): the trace as facts in onto-intelli, questions answered from them.
// Facts are entities `kind:key` of one onto-intelli workspace per nexc workspace: agent:{principal without
// agent://}, run:{id}, node:{id}, document:{id}, memory:{id}, tool:{name}, model:{name}, host:{host},
// policy:{agent key} (the agent's capabilities), action:{kind}/{subject} (what a policy decided on); links
// ran, executed, used_tool, used_model, read, called, delegated_to, governed_by, allows, denied, denied_in,
// attempted, reports_to; events agent.llm|tool|read|egress|delegated|approval, policy.denied, policy.changed,
// run.finished. Only ids, names, hosts, counts, decisions, labels, status and cost are exported, never content.
type AnswerMode = "deterministic" | "llm"; // from the graph alone (0 tokens), or a model explained with facts it was given
interface AuditFact {
  id: string; // the entity, or `withheld:{kind}:{n}` for data the asker is not cleared for
  kind: string; // agent | run | node | document | memory | policy | action | tool | host | model | withheld
  label: string;
  link: string | null; // where to open it in the console (/app/agents?agent=, /app/runs/{id}, /app/settings/knowledge, /app/settings/memory)
}
interface AuditAnswer {
  answer: string;
  mode: AnswerMode;
  facts: AuditFact[]; // what the answer rests on
  tokens: number; // model tokens spent (0 when deterministic)
  latency_ms: number; // question in to answer out
  unverified: boolean; // claims without a fact cited: a model's words, not the record
  question: string | null; // the canned question recognised: restricted_reads | egress_denied | run_denied | agent_day | code_policies | approvals
}
interface CannedQuestion {
  id: string;
  label: string; // `{run}` and `{agent}` are slots the asker fills
}

type AgentStatus = "active" | "paused" | "over_budget" | "quarantined";
type DataLabel = "public" | "internal" | "confidential" | "restricted"; // classification of data; the clearance an agent holds
interface Capabilities {
  // `null` on a list: no restriction. `{}` is the default policy. A sub-agent's set is the meet of its parent's and its own: never wider.
  tools: string[] | null; // tools the agent may call
  data: {
    topics: string[] | null; // knowledge topic ids whose passages it may read
    labels: DataLabel[]; // its clearance (default public, internal)
    memory_namespaces: ("user" | "graph" | "node")[] | null; // the scopes it may recall memories from
  };
  egress_hosts: string[] | null; // `api.example.com`, `*.example.com`
  counterparts: string[] | null; // roles of the sub-agents it may spawn
  delegation: { max_depth: number; max_fan_out: number }; // 0-8 levels below it, 0-32 children each (default 2, 4)
  models: string[] | null;
}
interface Agent {
  id: string;
  principal_id: string; // `agent://{workspace}/{name}`; follows the name
  name: string;
  role: string;
  title: string;
  model: string;
  system_prompt: string;
  reports_to: string | null;
  budget_tokens: number;
  spent_tokens: number;
  status: AgentStatus;
  runtime: AgentRuntime;
  heartbeat_at: string | null;
  created_at: string;
  owner_user_id: string | null; // who answers for it
  team_id: string | null;
  purpose: string;
  capabilities: Capabilities;
  policy_version: number; // raised with every change to the capabilities, and on quarantine and release
  quarantined_at: string | null;
  risk_score: number; // 0–10: how far its latest run strayed from its baseline (0 until three runs were learned from)
  risk_reasons: string[]; // in words: "3.2x its usual bytes out", "2 hosts it never called: a.example, b.example"
  risk_updated_at: string | null;
}
// `external`: the agent runs somewhere else and uses nexc as its policy and trace service through
// the Agent Firewall API (§8), with an API token issued in the console.
type AgentRuntime = "python" | "builtin" | "external";
interface AgentApiToken {
  id: string;
  agent_id: string;
  prefix: string; // the token's first characters, to tell tokens apart
  name: string;
  created_by: string | null;
  created_at: string;
  last_used_at: string | null;
  revoked_at: string | null;
}
interface IssuedAgentApiToken { token: AgentApiToken; secret: string } // `nexc_agt_…`, shown once

// MCP servers of a workspace (the tools gateway, §8). The backend connects to them on an agent's
// behalf; their tools are offered to agents as `mcp:<name>/<tool>` under the policy.
type McpTransport = "stdio" | "http";
interface McpTool { name: string; description: string; input_schema: object } // JSON Schema
interface McpServer {
  id: string;
  workspace_id: string;
  name: string; // [a-z0-9_-]{1,64}
  transport: McpTransport;
  command: string | null; // stdio: the executable, started for every call
  args: string[];
  url: string | null; // http: where the server listens
  has_auth_header: boolean; // an `Authorization` header is kept, encrypted, never shown
  allowed_tools: string[]; // empty: every tool the server lists
  max_label: DataLabel; // the strictest classification of data that may be sent to it
  tools: McpTool[]; // what it listed at the last connection test
  last_checked_at: string | null;
  last_check_ok: boolean | null;
  last_check_detail: string;
  created_by: string | null;
  created_at: string;
  updated_at: string;
}
interface McpServerInput {
  name: string; transport: McpTransport; command?: string; args?: string[]; url?: string;
  auth_header?: string; allowed_tools?: string[]; max_label?: DataLabel; // default `internal`
}
interface McpCheck { ok: boolean; detail: string; tools: McpTool[]; latency_ms: number }

// Alert routing: where a workspace's alerts and approvals go beyond the console. The URL is kept
// encrypted (a Slack incoming-webhook URL carries its token) and only its host is shown; the
// signing secret is generated once, shown once, kept encrypted.
type DestinationKind = "webhook" | "slack"; // a signed JSON POST | a Slack incoming webhook (Block Kit)
type DeliveryStatus = "pending" | "delivered" | "failed";
type DeliveryEvent = "alert.raised" | "approval.requested" | "approval.decided" | "test";
interface AlertDestination {
  id: string;
  workspace_id: string;
  name: string; // unique in the workspace, ≤100 characters
  kind: DestinationKind;
  host: string; // the URL's host; the URL itself is never shown again
  kinds: AlertKind[]; // alert kinds sent; empty: every kind
  min_severity: Severity; // the least severity sent
  include_approvals: boolean; // approvals requested and decided go too
  enabled: boolean;
  disabled_reason: string; // why the platform switched it off (50 deliveries in a row failed), else ""
  failures: number; // deliveries that failed in a row; reset by one that went through
  last_delivered_at: string | null;
  last_error: string; // what the last failed delivery said
  created_by: string | null;
  created_at: string;
  updated_at: string;
}
interface IssuedAlertDestination { destination: AlertDestination; secret: string } // `nexc_whs_…`, shown once
interface AlertDestinationInput {
  name: string; kind: DestinationKind; url: string; // http(s), no credentials, no private or loopback host
  kinds?: AlertKind[]; min_severity?: Severity; include_approvals?: boolean; enabled?: boolean; // defaults: [], low, false, true
}
interface AlertDestinationPatch { // a field left out stays; enabling clears `failures` and `disabled_reason`
  name?: string; url?: string; kinds?: AlertKind[]; min_severity?: Severity; include_approvals?: boolean; enabled?: boolean;
}
interface AlertDelivery { // one entry of a destination's log
  id: string; // the idempotency id the receiver saw
  destination_id: string;
  event: DeliveryEvent;
  alert_id: string | null;
  approval_id: string | null;
  status: DeliveryStatus;
  attempts: number;
  next_attempt_at: string | null; // while pending
  last_attempt_at: string | null;
  last_status: number | null; // the HTTP status of the last attempt, when one came back
  last_error: string;
  created_at: string;
  delivered_at: string | null;
}
interface DestinationTest { ok: boolean; status: number | null; detail: string; latency_ms: number }

// What a `webhook` destination receives: one `POST` per delivery, `Content-Type: application/json`,
// `X-Nexc-Signature: sha256=<hex HMAC-SHA256 of the raw body under the destination's secret>` and
// `X-Nexc-Timestamp: <unix seconds when it was sent>`. `id` is the delivery's idempotency id, the
// same on every retry; `timestamp` is when the thing happened. Free text (`alert.detail`,
// `approval.reason`, `approval.decision_reason`) is cut at 4,000 characters. 2xx is delivered;
// no answer, 408, 429 and 5xx are retried after 30 s, doubling up to an hour, for 24 hours;
// anything else (3xx included: redirects are not followed) fails for good. One attempt waits
// 10 s at most. A `slack` destination receives a Block Kit message instead, signed the same way:
// `{text, attachments: [{color, blocks}]}` with a header (the alert's kind and title, or the
// approval's tool and outcome), fields (severity, agent, rule, workspace), the detail, and link
// buttons into the console (no interactive callbacks).
interface DeliveryEnvelope {
  id: string;
  event: DeliveryEvent;
  timestamp: string;
  workspace: { id: string; name: string; slug: string };
  agent: { id: string | null; principal: string; name: string } | null; // null on a test
  alert?: Alert; // on alert.raised
  approval?: Approval; // on approval.requested / approval.decided (status says approved, denied or expired)
  message?: string; // on test
  links: { console: string; run: string | null; agent: string | null }; // under NEXC_PUBLIC_URL
}
// Verifying a delivery, in Python (Flask shown; any framework that gives the raw body works):
//
//     import hashlib, hmac
//
//     def verify(secret: str, body: bytes, signature: str) -> bool:
//         expected = "sha256=" + hmac.new(secret.encode(), body, hashlib.sha256).hexdigest()
//         return hmac.compare_digest(expected, signature)
//
//     @app.post("/hooks/nexc")
//     def hook():
//         if not verify(SECRET, request.get_data(), request.headers.get("X-Nexc-Signature", "")):
//             abort(401)
//         event = request.get_json()
//         seen_before = event["id"] in processed  # retries carry the same id
//         ...
//         return "", 204

interface AgentCard {
  agent: Agent;
  owner: { id: string; name: string } | null;
  team: { id: string; name: string; key: string } | null;
  budget_left: number | null; // null without a budget
  recent_spans: Span[]; // its last 20 spans across the runs the caller may open
  evaluation: AgentEvaluation | null; // the score of an agent made from a role template
}

// Role templates (AgentSecOps §4.6): a role with its capabilities, its own policy, the knowledge
// topics it is bound to (by term, where the workspace has them) and three competency questions,
// asked once with the workspace's provider when an agent is made from the template.
interface CompetencyQuestion { question: string; expected: string[] } // one expected term in the answer passes
interface AgentTemplate {
  id: string; // analyst | researcher | reviewer
  name: string;
  role: string;
  title: string;
  description: string;
  purpose: string;
  runtime: "python" | "builtin";
  budget_tokens: number;
  capabilities: Capabilities; // the model allow-list is pinned to the agent's model on creation
  policy: PolicyDocument; // becomes the agent's policy version 1
  knowledge: string[]; // topic terms
  questions: CompetencyQuestion[];
}
interface AgentEvaluation {
  template: string;
  score: number; // questions passed
  total: number;
  model: string; // the model that answered
  questions: { question: string; expected: string[]; passed: boolean; answer: string }[]; // `answer`: the first 300 characters
  evaluated_at: string;
}

// The AgentSecOps console (phase 5). Runs, spans, denials and artifacts count only what the caller may open.
interface SecopsOverview {
  agents: { total: number; active: number; paused: number; over_budget: number; quarantined: number };
  runs_today: RunCounts; // created since midnight UTC
  runs_week: RunCounts; // created in the last seven days
  spans_today: number;
  denials_today: { rule: string; count: number }[]; // policy spans with decision `deny`, by rule, most first
  denials_fortnight: { rule: string; count: number }[];
  approvals_open: number; // pending
  alerts_open: { severity: Severity; count: number }[]; // unacknowledged, worst first
  artifacts_quarantined: number; // kept in and not released
  top_risk: { agent_id: string; name: string; principal_id: string; status: AgentStatus; risk_score: number; risk_reasons: string[]; risk_updated_at: string | null }[]; // five, highest first
  policy_versions: { scope: PolicyScope; scope_id: string | null; version: number; created_at: string }[]; // the latest at every level that has one
  last_seal: { run_id: string; graph_id: string; trace_hash: string; sealed_at: string | null } | null;
  risk_history: { agent_id: string; day: string; score: number }[]; // highest score per agent per UTC day, last 14 days, oldest first (kept by a trigger on `agents.risk_updated_at`)
  at: string;
}
interface RunCounts { total: number; succeeded: number; failed: number; cancelled: number; active: number }
interface SecopsSpan { // a span as the trace explorer lists it: the ids and the attributes the table shows
  id: string; run_id: string; graph_id: string; node_id: string; agent_id: string | null; principal: string;
  kind: SpanKind; name: string; started_at: string; ended_at: string | null; status: SpanStatus;
  tool: string | null; // `attributes.tool`, or the name of a tool call
  host: string | null; // `attributes.egress_host`
  decision: string | null; // `attributes.decision`
  rule: string | null; // `attributes.rule`
  model: string | null; // `attributes.model`
  tokens_in: number | null; tokens_out: number | null;
}

// Versioned policies (AgentSecOps phase 2). A document only says what it sets; what it leaves out
// (`null`, `[]`, `{}`) is inherited from the level above. Resolution walks workspace -> team -> agent
// over the agent's own `Capabilities` and only narrows: allow-lists are met, deny-lists added,
// labels intersected, limits lowered, approvals and alert thresholds added, DLP and injection
// actions tightened. A version that would widen the levels above is refused (422, `document.<rule>`).
type PolicyScope = "workspace" | "team" | "agent";
type RuleAction = "alert" | "deny";
type Severity = "low" | "medium" | "high" | "critical";
type PiiKind = "email" | "phone" | "card" | "national_id";
type InjectionAction = "flag" | "block";
interface ListRule { allow: string[] | null; deny: string[] } // `allow: null` inherits
interface TimeWindow { days: number[]; start: string; end: string } // ISO weekdays (1 = Monday), `HH:MM` UTC; an end before the start wraps midnight
interface PolicyDocument {
  tools: ListRule;
  egress: ListRule; // hosts: `api.example.com`, `*.example.com`
  models: ListRule;
  labels: DataLabel[] | null; // the clearance agents under the policy may hold
  limits: {
    tokens_per_run: number | null; // caps the budget the runtime is given
    tokens_per_day: number | null; // across runs, UTC day, checked before dispatch
    cost_usd_per_day: number | null;
    tool_calls_per_minute: number | null; // within one run, enforced by the runtime
    runs_per_hour: number | null; // node starts per agent, per backend instance
    max_depth: number | null;
    max_fan_out: number | null;
  };
  time_windows: TimeWindow[] | null; // when work may start; `[]` never
  require_approval: {
    tools: string[]; // wait for a human before they run
    labels: DataLabel[]; // a node given data of these labels needs an approval before `write_file` or `make_docx`
    approvers: string[]; // member ids; empty: every workspace admin
    timeout_s: number | null; // 1–86400; default 600; the node fails when it runs out
  };
  alerts: {
    risk_score: number | null; // at and above: an alert
    quarantine_risk_score: number | null; // at and above: the agent is quarantined
    denials_per_run: number | null;
    novelty: number | null; // 0–1: how unlike every earlier run of the agent
  };
  dlp: {
    secrets: RuleAction | null; // a credential in an artifact (default deny)
    pii: { enabled: boolean | null; kinds: PiiKind[] | null; keywords: string[]; action: RuleAction | null }; // default on, every kind, alert
    labels: Partial<Record<DataLabel, { artifacts: boolean | null; egress_hosts: string[] | null }>>; // where content derived from a label may go (defaults: confidential stays an artifact and never leaves to a host; restricted leaves in no form)
  };
  injection: { action: InjectionAction | null; model_check: boolean | null }; // default flag, no model check
}
interface Policy {
  id: string;
  workspace_id: string;
  scope: PolicyScope;
  scope_id: string | null; // the team or agent; null at the workspace level
  version: number; // 1, 2, ... per scope
  document: PolicyDocument;
  author_id: string | null;
  author_name: string;
  created_at: string;
}
interface PolicyVersions { workspace: number | null; team: number | null; agent: number | null }
// The resolved rules beside the capabilities, carried in the per-run token (§8) and evaluated the
// same way on both sides. `Rules` with nothing set is what an agent gets when no policy exists.
interface Rules {
  tools_denied: string[];
  egress_denied: string[];
  models_denied: string[];
  require_approval: { tools: string[]; labels: DataLabel[]; approvers: string[]; timeout_s: number };
  limits: { tokens_per_run: number | null; tokens_per_day: number | null; cost_usd_per_day: number | null; tool_calls_per_minute: number | null; runs_per_hour: number | null };
  time_windows: TimeWindow[][]; // one set per level that set some; a time must be in a window of every set
  alerts: { risk_score: number | null; quarantine_risk_score: number | null; denials_per_run: number | null; novelty: number | null };
  dlp: { secrets: RuleAction; pii: { enabled: boolean; kinds: PiiKind[]; keywords: string[]; action: RuleAction }; labels: Record<DataLabel, { artifacts: boolean; egress_hosts: string[] | null }> };
  injection: { action: InjectionAction; model_check: boolean };
  versions: PolicyVersions;
}
type ApprovalStatus = "pending" | "approved" | "denied" | "expired";
interface Approval {
  id: string;
  workspace_id: string;
  run_id: string;
  node_id: string;
  graph_id: string;
  agent_id: string | null;
  principal: string; // who asked
  span_id: string; // the policy span that asked (decision require_approval)
  tool: string; // what waits
  args_sha256: string | null; // a digest of its arguments, never the arguments
  rule: string; // approvals.tools | approvals.labels
  reason: string;
  approver_ids: string[]; // empty: every workspace admin
  status: ApprovalStatus;
  requested_at: string;
  expires_at: string; // the call stops waiting here
  decided_by: { id: string | null; name: string } | null;
  decided_at: string | null;
  decision_reason: string;
  decision_span_id: string | null; // the `approval` span, once recorded
  policy_versions: PolicyVersions;
}
interface ResolvedPolicy {
  capabilities: Capabilities; // the agent's, narrowed by every level: what the token carries
  rules: Rules;
  origins: Record<string, "workspace" | "team" | "agent" | "capabilities">; // rule path (`tools.allow`, `limits.tokens_per_run`, `dlp.labels.restricted.artifacts`, …) -> the level that set it; rules at their default are absent
  levels: { scope: PolicyScope; scope_id: string | null; version: number }[]; // outermost first
}

type MemoryKind = "fact" | "experience" | "observation" | "preference";
interface Memory {
  id: string;
  scope: "user" | "graph" | "node";
  graph_id: string | null;
  node_id: string | null;
  kind: MemoryKind;
  content: string;
  importance: number;
  access_count: number;
  score: number | null;
  created_at: string;
  updated_at: string;
  label: DataLabel; // only agents cleared for it recall the memory
}

interface OrchestratorStatus {
  demo_mode: boolean;
  queue_depth: number;
  running_nodes: number;
  active_runs: number;
  agents_active: number;
  backends: {
    llm: BackendHealth;
    agent_runtime: BackendHealth;
    symphony: BackendHealth;
  };
}
interface BackendHealth {
  enabled: boolean;
  ok: boolean;
  url: string | null;
  detail: string | null;
}
```

## 5. Endpoints (`/api/v1`)

Public: `GET /healthz`, `GET /readyz`, `POST /auth/register`, `POST /auth/login`, `POST /auth/refresh`,
`POST /auth/logout`, `POST /auth/password/reset`, `GET /api/openapi.json`, `GET /api/docs` (Scalar UI). Everything else needs a bearer token.

| Method & path                                                                                                               | Body                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Response                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| POST `/auth/register`                                                                                                       | `{email, password (≥12 chars), name}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | 201 `AuthResponse` + refresh cookie (403 if signups disabled)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| POST `/auth/login`                                                                                                          | `{email, password, code?}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | 200 `AuthResponse` + cookie (401 generic message; 422 with `errors.code` when the account has two-factor sign-in and `code` is missing or wrong, a code works once; 429 after 5 fails/min/IP)                                                                                                                                                                                                                                                                                                                                                                                                   |
| POST `/auth/refresh`                                                                                                        | — (cookie + `X-Requested-With`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | 200 `AuthResponse` + rotated cookie                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| POST `/auth/logout`                                                                                                         | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 204, cookie cleared, family revoked                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| GET `/auth/me`                                                                                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `User`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| PATCH `/auth/me`                                                                                                            | `{name}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | `User`. Either side of the platform boundary, like every `/auth/*` route below                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| POST `/auth/password`                                                                                                       | `{current_password, new_password (≥12 chars)}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | 200 `AuthResponse` + new refresh cookie. Every session ends on every device and this one starts anew. 422 for a wrong current password or a new one that does not meet the rules; wrong passwords count as failed sign-ins from the address (429)                                                                                                                                                                                                                                                                                                                                               |
| POST `/auth/password/reset`                                                                                                 | `{token, new_password (≥12 chars)}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | 204; public. Sets a new password with the token of a reset link a platform administrator issued. The link works once and for an hour; every session of the account ends. 401 for an unknown, used, replaced or expired link (counted like a failed sign-in: 429 after five in a minute); 422 for a password that does not meet the rules, which does not use the link up                                                                                                                                                                                                                        |
| GET `/auth/sessions`                                                                                                        | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `{active}`: sign-ins that are still alive, this one included                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| GET `/auth/activity?limit=`                                                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | The caller's security activity, newest first (`limit` 1-100, default 30): `{id, kind: registered\|signed_in\|sign_in_failed\|password_changed\|password_reset\|reset_link_issued\|sessions_ended\|suspended\|reactivated\|role_changed, ip\|null, detail, created_at}[]`. `ip` is where the holder's own request came from; what a platform administrator did has none and says so in `detail`. Kept 180 days                                                                                                                                                                                   |
| GET `/auth/2fa`                                                                                                             | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `{enabled, recovery_codes_left}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| POST `/auth/2fa/setup`                                                                                                      | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `{secret, uri}`: a secret for an authenticator app (TOTP, RFC 6238: SHA-1, 6 digits, 30 s) as base32 text and as an `otpauth://` address. Not in force until enabled. 409 when already on                                                                                                                                                                                                                                                                                                                                                                                                       |
| POST `/auth/2fa/enable`                                                                                                     | `{code}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | `{recovery_codes[8]}`, shown once. From then on `POST /auth/login` needs `code`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| POST `/auth/2fa/disable`                                                                                                    | `{password, code}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 204. `code` is the app's code or a recovery code                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| POST `/auth/sessions/end`                                                                                                   | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 204; signs out everywhere, this device included (access tokens stop at once)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| GET `/auth/me/export`                                                                                                       | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | A JSON file (`nexc-account.json`): `{exported_at, account: User, sections: {workspaces, teams, issues_created, issues_assigned, comments, graphs, documents, memories, ai_account, usage, security_activity, notifications}, truncated[]}`. Each section is a list of plain objects, cut at 5,000 rows (named in `truncated`). Never a password hash or a key                                                                                                                                                                                                                                   |
| POST `/auth/me/delete`                                                                                                      | `{password}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | 204. Deletes the caller's account by emptying it: workspaces it is alone in are removed with their files, it leaves the others (assignments and project leads cleared, their audit logs note it), and its name, address, password, sessions, AI account, notifications and personal memories are gone; what it made stays under "Deleted account" audit logs and the platform's activity log keep their entries with the name and address replaced, and the address can register again. 409 for the only owner of a shared workspace and for a platform administrator; 422 for a wrong password |
| GET `/settings/llm?workspace_id=`                                                                                           | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `LlmSettings` in effect for the caller (in that workspace)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| PUT/DELETE `/settings/llm`                                                                                                  | PUT `{provider, model, base_url?, api_key?}` (`api_key: ""` deletes the key)                                                                                                                                                                                                                                                                                                                                                                                                                                        | `LlmSettings` / 204: connects or disconnects the caller's own account                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| GET `/settings/llm/models?workspace_id=`                                                                                    | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `ModelCatalog`: what the effective provider's own `/models` endpoint lists; 422 if it cannot be reached                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| GET/PUT/DELETE `/workspaces/{wid}/llm`                                                                                      | PUT as above (admin+)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | `LlmSettings \| null` (any member) / `LlmSettings` / 204                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| GET/POST `/workspaces`                                                                                                      | POST `{name}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `Workspace[]` / 201 `Workspace` (caller becomes owner)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| GET/PATCH/DELETE `/workspaces/{wid}`                                                                                        | PATCH `{name}` (admin+)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | `Workspace` / 204 (owner; 409 for the caller's only workspace)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| GET/POST `/workspaces/{wid}/members`                                                                                        | POST `{email, role?}` (admin+; `owner` only by owners)                                                                                                                                                                                                                                                                                                                                                                                                                                                              | `WorkspaceMember[]` (not guests) / 201 `{member, invite}`: registered users join at once, others get an invite that is accepted on sign-up                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| PATCH/DELETE `/workspaces/{wid}/members/{uid}`                                                                              | PATCH `{role}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | `WorkspaceMember[]` / 204; only owners change owners; 409 if no owner would remain; anyone may remove themselves                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| GET/POST `/agents?workspace_id=` | POST `{name, role, title?, model?, system_prompt?, reports_to?, budget_tokens?, runtime?, workspace_id?, purpose?, owner_user_id?, team_id?, capabilities?}` | `Agent[]` of the workspace (any member) / 201 `Agent` (member+; guests 403). The owner defaults to the caller and must be a member; the team one of the workspace's; the capabilities the default policy. A new workspace starts with a default organisation, each agent with a principal and the default policy |
| GET/PATCH/DELETE `/agents/{id}` | PATCH: any subset of the agent fields (`team_id: null` detaches; `status` may be `active` or `paused`; `capabilities` replaces the whole document) | GET `AgentCard` (any member) / `Agent` / 204 (member+ of the agent's workspace; others 404). A change of capabilities raises `policy_version` and is audited (`policy_changed`); a quarantined agent's status is changed only through release (422) |
| GET `/agents/{id}/activity?limit=` | — | `Span[]`: the agent's most recent spans across runs, newest first, `limit` 1–200 (default 20); only spans of runs whose graph the caller may open (any member of the agent's workspace; others 404) |
| GET/POST `/workspaces/{wid}/policies?scope=&scope_id=&limit=` | POST `{scope: PolicyScope, scope_id?, document: PolicyDocument}` | `Policy[]` newest first, `limit` 1–200 (default 50), narrowed to one level by `scope` and `scope_id` / 201 `Policy` (workspace admins; members 403; others 404). The new version is the next number at its level; `scope_id` must be a team or agent of the workspace (422 `scope_id`); a document that would widen what the levels above resolve to is refused (422, `document.<rule>`: "would widen the policy of the level above"). Audited as `policy_changed` (subject: `the workspace`, `team {id}` or the agent's principal; detail: the version and what changed since the previous one, rule by rule) |
| GET `/workspaces/{wid}/policies/resolve?agent_id=` | — | `ResolvedPolicy` of the agent (any member; unknown agent 404): its capabilities narrowed by every level, the rules beside them, which level set each rule, and the versions in force |
| GET `/workspaces/{wid}/approvals?status=&limit=` | — | `Approval[]` newest first (admins), `limit` 1–200 (default 50) |
| GET `/approvals/{id}` | — | `Approval` (members of its workspace; others 404) |
| GET `/workspaces/{wid}/alerts?before=&limit=` | — | `Alert[]` newest first (admins), `before` pages by `created_at`, `limit` 1–200 (default 50) |
| POST `/alerts/{id}/acknowledge` | — | `Alert` (admins of its workspace; others 404; 409 once acknowledged) |
| GET `/workspaces/{wid}/alerts/export?since=&until=&format=` | — | `Alert[]` oldest first, streamed as `application/x-ndjson` (one object per line; `format=json` for one array): the way a SIEM pulls without webhooks (admins; members 403; outsiders 404). `since` is exclusive (default: 24 h before `until`), `until` inclusive (default: now); 422 when the window is the wrong way round. At most 10,000 per pull: a pull that answered that many has more, from `since` = the last `created_at`. 30 pulls a minute per account, burst 10 (429 with `Retry-After`) |
| GET/POST `/workspaces/{wid}/alert-destinations` | POST `AlertDestinationInput` | `AlertDestination[]` by name / 201 `IssuedAlertDestination` (admins; members 403; outsiders 404; 409 on a name already taken; 422 on `name` or `url`: not http(s), credentials in it, or a private or loopback host unless `NEXC_ALERT_ALLOW_PRIVATE_HOSTS`; at most 32 destinations). The secret is in the answer once. Audited as `alert_destination_changed` (subject `alerts:<name>`) |
| GET/PATCH/DELETE `/workspaces/{wid}/alert-destinations/{id}` | PATCH `AlertDestinationPatch` | `AlertDestination` / `AlertDestination` / 204 (admins; members 403; others 404; PATCH 409 on a name taken, 422 on a URL that does not fit). Enabling a destination the platform switched off clears `failures` and `disabled_reason`; removal takes the delivery log with it; every change is audited |
| POST `/workspaces/{wid}/alert-destinations/{id}/test` | — | `DestinationTest` (admins): sends a `test` event at once and says what the destination answered; logged, audited, not counted toward switching the destination off |
| GET `/workspaces/{wid}/alert-destinations/{id}/deliveries?limit=` | — | `AlertDelivery[]` newest first, `limit` 1–200 (default 50) (admins): the delivery log, test events included |
| GET `/workspaces/{wid}/secops/overview` | — | `SecopsOverview` (members and above; guests 403; outsiders 404): what the console's top of page shows, in one call |
| GET `/workspaces/{wid}/secops/spans?agent=&kind=&tool=&host=&run=&from=&to=&decision=&limit=&before=` | — | `{spans: SecopsSpan[], next: string \| null}` newest first (by id) across every run the caller may open (members and above); `kind` is a `SpanKind`, `decision` one of `allow`, `deny`, `require_approval`, `alert`, `tool` matches `attributes.tool` or the name of a tool call, `host` matches `attributes.egress_host`, `from`/`to` bound `started_at` (RFC 3339, `from` before `to`), `limit` 1–500 (default 100), `before` continues below the previous page's `next`. Unknown kinds, decisions, ids and times are 400 or 422 |
| GET `/workspaces/{wid}/secops/events` | — | `text/event-stream` (members and above; bearer authenticated, no ticket: read it with `fetch`): every `trace.span` and `alert` event (§6 frames) of every graph the caller may open, as one feed, with `retry: 3000` first and a `heartbeat` every 15 s; graphs created while it is open are picked up within ten seconds. Ends when the account's sessions are ended |
| POST `/artifacts/{aid}/release` | — | `Artifact` (admins of the run's workspace; members 403; others 404; 409 when not quarantined): a quarantined artifact can be downloaded again. Audited as `artifact_released` |
| POST `/approvals/{id}/approve`, POST `/approvals/{id}/deny` | `{reason?}` (a reason is required to deny) | `Approval` (an approver the policy names, or a workspace admin; other members 403; outsiders 404; 409 once decided). The decision reaches the runtime holding the call within about a second, is recorded as an `approval` span of the run (`decision`, `decided_by`, `decided_by_name`, `reason`, `request_span_id`, `waited_ms`) and audited as `approval_granted` / `approval_denied` (subject: the principal; detail: the tool, the run and the reason); the approvers' inbox entries are marked read |
| GET `/agent-templates` | — | `AgentTemplate[]`: the built-in role templates (Analyst, Researcher, Reviewer) |
| POST `/agents/from-template/{name}` | `{workspace_id?, name?, model?, owner_user_id?, team_id?, evaluate?}` (every field optional; `evaluate` defaults to true) | 201 `AgentCard` (members and above; guests 403; outsiders 404; unknown template 404; an agent of that name 409). The agent gets the template's capabilities with its model allow-list pinned to `model` (default: the server's; the model the workspace's provider resolves to is admitted too, and `nexc-demo` under the demo provider), its knowledge topics bound where the workspace has topics with those terms, and the template's policy as its agent-level version 1 (audited as `policy_changed`; 422 when it would widen the levels above). With a usable provider and `evaluate`, the three competency questions are asked with the agent's system prompt (usage purpose `evaluation`) and scored on `evaluation`; without one, or when the model does not answer, the agent exists and `evaluation` is `null` |
| POST `/agents/{id}/quarantine`, POST `/agents/{id}/release` | — | `Agent` (workspace admins; members 403; others 404). Quarantine cancels the agent's running nodes (they fail, with a `policy` span of rule `quarantine`), refuses new ones and raises `policy_version` so no earlier token is honoured; release puts it back in service (`active`, or `over_budget`). Audited as `agent_quarantined` / `agent_released`; 409 when already in that state |
| GET/POST `/agents/{id}/tokens` | POST `{name?}` | `AgentApiToken[]` / 201 `IssuedAgentApiToken` (workspace admins; members 403; others 404; 422 unless the agent's runtime is `external`). The secret is in the answer once and kept as a SHA-256 hash; audited as `agent_token_issued` (subject the principal, detail the prefix) |
| DELETE `/agents/{id}/tokens/{tid}` | — | 204 (workspace admins; 404 for a token already revoked). The token is refused by the Agent Firewall API from the next call on; audited as `agent_token_revoked` |
| GET/POST `/workspaces/{wid}/mcp-servers` | POST `McpServerInput` | `McpServer[]` (members and above) / 201 `McpServer` (admins; guests and members 403; outsiders 404; 409 on a name already registered; 422 on an input that does not fit, `name`, `command`, `url`, `auth_header`, `allowed_tools`, `args`; at most 64 servers). Registering connects at once (`initialize`, `tools/list`, 15 s) and records the tools or the failure (`last_check_ok`, `last_check_detail`); audited as `mcp_server_added` (subject `mcp:<name>`) |
| GET `/workspaces/{wid}/mcp-servers/{sid}`, DELETE | — | `McpServer` (members) / 204 (admins; others 404). Removal is audited as `mcp_server_removed`; the server's tools stop being offered at the next run |
| POST `/workspaces/{wid}/mcp-servers/{sid}/check` | — | `McpCheck` (admins): connects again and records what the server lists |
| GET `/memories?workspace_id=&graph_id=&q=&limit=&offset=&preview=`                                                          | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Memory[]`: what the workspace's graphs learned, limited to graphs the caller can open, plus the caller's own notes; with `q` a hybrid search with `score`. `graph_id` narrows to one graph                                                                                                                                                                                                                                                                                                                                                                                                     |
| GET `/memories/{id}`                                                                                                        | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Memory` in full, for those who may read it (404 otherwise). Lists page with `offset` (0-5000) and may carry previews: `preview=N` cuts each `content` to N characters (20-2000) and ends it with `…`                                                                                                                                                                                                                                                                                                                                                                                           |
| GET `/memories/topics?workspace_id=`                                                                                        | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `MemoryTopic[]` largest first: `{id, label, terms, memory_count}` (memories of the topic the caller may read; topics with none for them are left out). Found by clustering memory embeddings; named only from memories the whole workspace can read. `GET /memories` takes `topic_id`, and `Memory` carries `topic_id\|null`                                                                                                                                                                                                                                                                    |
| POST `/memories/topics/rebuild?workspace_id=`                                                                               | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 202; finds the topics afresh in the background (admins and owners)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| GET `/workspaces/{wid}/documents?q=&limit=&offset=` | — | `Document[]` newest first: `{id, workspace_id, name, size_bytes, status: pending\|parsing\|embedding\|ready\|failed, error, page_count\|null, chunk_count, uploaded_by\|null, created_at, updated_at, label: DataLabel}`. Everyone but guests |
| POST `/workspaces/{wid}/documents?name=`                                                                                    | the file's bytes, `application/octet-stream`, at most 50 MiB                                                                                                                                                                                                                                                                                                                                                                                                                                                        | 201 `Document` (`pending`; parsed, split into passages and embedded in the background), or 200 with the existing document when the workspace already has these bytes                                                                                                                                                                                                                                                                                                                                                                                                                            |
| GET/PATCH/DELETE `/workspaces/{wid}/documents/{did}` | PATCH (admins) `{label: DataLabel}` | `Document` / `Document` / 204 (delete: whoever uploaded it, or an admin). Classifying is audited as `classification_changed`; only agents cleared for the label are given the document's passages |
| GET `/workspaces/{wid}/knowledge/search?q=&topic_id=&limit=`                                                                | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Passage[]` best first (1-20, default 8): `{chunk_id, document_id, document_name, page\|null, section_path, kind: text\|table, content, topic_id\|null, score}`. Candidates are keyword matches (the query's rarest words) plus, when the database has pgvector, the query's nearest neighbours (at least 0.3 similar); they are ordered by fusing keyword rank with embedding similarity                                                                                                                                                                                                       |
| GET `/workspaces/{wid}/knowledge/topics` | — | `Topic[]` largest first: `{id, label, terms, chunk_count, classification: DataLabel}`. Found without supervision: passages are clustered by their embeddings and each cluster is named by the words that set it apart. They appear once a workspace has about twenty passages; new passages join the nearest topic |
| PATCH `/workspaces/{wid}/knowledge/topics/{tid}` | (admins) `{classification: DataLabel}` | `Topic`. Every passage under the topic takes the stricter of its document's label and this; audited as `classification_changed` |
| POST `/workspaces/{wid}/knowledge/topics/rebuild`                                                                           | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 202; finds the topics afresh in the background (admins and owners)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| GET/PUT `/workspaces/{wid}/knowledge/settings`                                                                              | PUT (admins) `{embed_base_url, embed_model, embed_dims, api_key?, passages, budget_chars, use_in_nodes, use_in_plan}`                                                                                                                                                                                                                                                                                                                                                                                               | `KnowledgeSettings` `{embed_base_url\|null, embed_model, embed_dims\|null, has_api_key, key_hint\|null, semantic, passages, budget_chars, use_in_nodes, use_in_plan}`. Changing the model re-embeds ready documents in the background                                                                                                                                                                                                                                                                                                                                                           |
| PATCH/DELETE `/memories/{id}` | PATCH (admins of the memory's workspace) `{label: DataLabel}` | `Memory` / 204 (delete: the author or anyone who can work on the memory's graph). Classifying is audited as `classification_changed` |
| GET `/workspaces/{wid}/issues?team_id=&assignee_id=&creator_id=&project_id=&label_id=&parent_id=&cycle_id=&open=&q=&limit=` | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Issue[]` in teams the caller can see, most recently updated first                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| POST `/workspaces/{wid}/teams/{tid}/issues`                                                                                 | `{title, description?, state_id?, priority?, assignee_id?, agent_id?, project_id?, label_ids?, parent_id?, cycle_id?, due_date? (YYYY-MM-DD)}`                                                                                                                                                                                                                                                                                                                                                                      | 201 `Issue`. Team members, and workspace members (not guests) for a team they can see                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| GET/PATCH/DELETE `/issues/{iid}`                                                                                            | PATCH any subset; `assignee_id`, `agent_id`, `project_id`, `due_date` accept `null`; `label_ids` replaces the issue's labels (at most 20); `parent_id` makes it a sub-issue of another issue of the workspace (`null` undoes it, loops are refused). `Issue` carries `parent: {id, identifier, title}\|null` (null when the caller cannot see the parent's team) and `sub_issues: {total, closed}`; `cycle_id` plans it in a cycle of its team (`null` takes it out), returned as `cycle: {id, number, name}\|null` | `Issue` / 204                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| POST `/issues/{iid}/graph`                                                                                                  | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Issue` with `graph_id`: creates a graph of the issue's team whose goal is the issue (team members only, 403 otherwise); idempotent                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| GET `/issues/{iid}/events`                                                                                                  | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `IssueEvent[]`, oldest first: comments and the changes to state, priority, assignee, title and due date. `{id, issue_id, kind: comment\|state\|priority\|assignee\|title\|due, actor: {user_id, name}\|null, body, from, to, created_at, edited_at}`                                                                                                                                                                                                                                                                                                                                            |
| POST `/issues/{iid}/comments`                                                                                               | `{body}` (1 byte - 16 KiB)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | 201 `IssueEvent`. Whoever may edit the issue                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| PATCH/DELETE `/issues/{iid}/comments/{cid}`                                                                                 | PATCH `{body}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | `IssueEvent` / 204. Only the author edits; the author or whoever manages the team deletes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| GET `/workspaces/{wid}/inbox`                                                                                               | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | The caller's last 100 `Notification`s, newest first: `{id, kind: assigned\|comment\|state\|due\|approval\|alert, issue: {id, identifier, title}\|null, approval: {id, run_id, node_id, graph_id, principal, tool, reason, status, expires_at}\|null, actor: {user_id, name}\|null, created_at, read_at}`. A member is told when an issue is assigned to them, and when someone else comments on or moves an issue they created or are assigned; an approver (every admin, or the members a policy names) is asked to decide a held tool call (`approval`, with the `approval` to open); admins hear alerts                                                                                                                                                                                                                                                                       |
| POST `/workspaces/{wid}/inbox/read`                                                                                         | `{ids?}` (all when left out)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | The inbox after marking                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| GET/POST `/workspaces/{wid}/labels`                                                                                         | POST `{name, color}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | `Label[]` by name / 201 `Label` `{id, name, color}`. Everyone but guests adds; names are unique ignoring case (409)                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| PATCH/DELETE `/workspaces/{wid}/labels/{lid}`                                                                               | PATCH `{name?, color?}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | `Label` / 204. Everyone but guests edits; workspace admins delete (issues lose the label)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| GET/POST `/workspaces/{wid}/teams/{tid}/cycles`                                                                             | POST `{name?, starts_on, ends_on}` (days, both included, at most 90)                                                                                                                                                                                                                                                                                                                                                                                                                                                | `Cycle[]` latest first / 201 `Cycle` `{id, team_id, number, name, starts_on, ends_on, status: upcoming\|active\|completed, issue_count, closed_count, created_at}`. Team owners and workspace admins plan; cycles of a team do not overlap (409)                                                                                                                                                                                                                                                                                                                                                |
| PATCH/DELETE `/workspaces/{wid}/teams/{tid}/cycles/{cid}`                                                                   | PATCH `{name?, starts_on?, ends_on?}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | `Cycle` / 204. Deleting leaves the issues, in no cycle                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| GET/POST `/workspaces/{wid}/teams/{tid}/states`                                                                             | POST `{name, category, color, position?}` (team owner or workspace admin)                                                                                                                                                                                                                                                                                                                                                                                                                                           | `IssueState[]` / 201 `IssueState`; 409 name taken                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| PATCH/DELETE `/workspaces/{wid}/teams/{tid}/states/{sid}`                                                                   | PATCH any subset                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | `IssueState` / 204; 409 while issues are in the state or it is the last one                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| GET/POST `/workspaces/{wid}/projects`                                                                                       | POST `{name, description?, target_date?}` (member+)                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | `Project[]` / 201 `Project`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| PATCH/DELETE `/workspaces/{wid}/projects/{pid}`                                                                             | PATCH `{name?, description?, status?, lead_id?, target_date?}` (member+)                                                                                                                                                                                                                                                                                                                                                                                                                                            | `Project` / 204 (admin+; issues stay)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| GET/PUT `/workspaces/{wid}/guardrails`                                                                                      | PUT `Guardrails` (admin+; omitted fields take their defaults)                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `Guardrails` `memory_limit` (>= 100) and `memory_forget_after_days` (>= 7), both `null` by default, let memory forget: untouched memories first, then the least important and least recalled beyond the limit; applied hourly                                                                                                                                                                                                                                                                                                                                                                   |
| GET `/workspaces/{wid}/audit?before=&limit=` | — | `AuditEntry[]` newest first (admins and owners): `{id, action, actor_id\|null, actor_name, subject, detail, created_at}`. Records changes to members, invitations, teams and their members, the workspace credential (provider and model, never the key), guardrails and deleted labels, the seal of every finished run's trace (`trace_sealed`: subject the run id, detail the span count and the chain's final hash), an agent's capabilities (`policy_changed`, subject its principal, detail the policy version), quarantine and release (`agent_quarantined`, `agent_released`), classifications (`classification_changed`), every action the runtime reported that a run's policy did not allow (`policy_mismatch`: subject the principal, detail the run, the span and the rule; the actor is whoever started the run), every policy version (`policy_changed`, subject the level, detail the diff), every decided approval (`approval_granted`, `approval_denied`: subject the principal, detail the tool, the run and the reason), every artifact the DLP classifier kept in or an admin released (`artifact_quarantined`, `artifact_released`), every alert (`alert_raised`: subject the principal, detail the severity, kind and title), every MCP server registered or removed (`mcp_server_added`, `mcp_server_removed`: subject `mcp:<name>`) and every API token of an external agent issued or revoked (`agent_token_issued`, `agent_token_revoked`: subject the principal, detail the prefix). `before` pages by `created_at`; `limit` 1-200, default 100 |
| GET `/workspaces/{wid}/audit/export?since=&until=&format=` | — | `AuditEntry[]` oldest first, the same shape, window, paging and rate limit as `/alerts/export`: the way a SIEM pulls the audit log without webhooks (admins; members 403; outsiders 404) |
| POST `/workspaces/{wid}/audit/ask` | `{question}` (1–1,000 characters) | `AuditAnswer` (members and above; guests 403; outsiders 404; 422 for an empty or oversized question; 503 problem+json when no onto-intelli is configured or it does not answer, 403 when it refuses the principal). The six canned questions (`GET …/audit/questions`) are recognised with their slots (a run id, an agent's name, a time window: today, this week, this month, last N days) and answered as graph and event queries: `deterministic`, 0 tokens, milliseconds. Any other question goes to onto-intelli's `IntelliService/Ask` (mode auto): its facts are resolved to nexc things and an answer that cites none is `unverified`. The asker's role is their clearance: owners and admins see every fact; members do not see confidential or restricted document and memory ids (the fact is `withheld` and the id is replaced in the text) |
| POST `/workspaces/{wid}/audit/ask/stream` | `{question}` | `text/event-stream`: `stage` `{text}` and `delta` `{text}` while onto-intelli explains, then `done` `AuditAnswer`, or `error` `{detail}`; same rules as the ask. A canned question, and any question from a member (the clearance applies to the text), arrives as one `done` |
| GET `/workspaces/{wid}/audit/questions` | — | `CannedQuestion[]`: the six chips in the order the console offers them (any member) |
| GET `/workspaces/{wid}/search?q=&limit=`                                                                                    | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Any member. What in the workspace matches `q` (2-100 chars) and the caller may see: `{kind: issue\|project\|graph\|document\|team\|member, id, title, subtitle}[]`, in that order, up to `limit` (1-10, default 5) of each kind. Issues match by title or identifier, people by name or e-mail. Guests get no documents, graphs of the workspace or members; a private team's issues stay with the team. Fewer than two characters match nothing                                                                                                                                                |
| GET `/workspaces/{wid}/timeline?day=`                                                                                       | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `TimelineEntry[]` for one day (UTC, default today), newest first (admins and owners): `{at, kind, actor\|null, entity_type, entity_id\|null, title, detail}`. Unifies the audit log, issues and their events, graphs, runs, documents and memories learned. `kind` is an open string                                                                                                                                                                                                                                                                                                            |
| GET `/workspaces/{wid}/timeline/days?days=`                                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `{day, events}[]` for the last days (1-366, default 30), latest first, empty days left out                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| GET/POST `/workspaces/{wid}/timeline/summary?day=`                                                                          | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Admins and owners. GET: the kept `DaySummary` of the day (UTC, today by default) or `null`; spends nothing. POST: the workspace's model writes it anew from the day's timeline and it is kept: `{day, headline, highlights[], attention[], event_count, stale, model, created_by\|null, created_at}`; `stale` when more has happened since. 409 for a day on which nothing happened, 403 when the guardrails refuse, 422 without a usable AI account. Booked as `summary` usage                                                                                                                 |
| GET `/workspaces/{wid}/map`                                                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `WorkspaceMap` (admins and owners): `{entities: [{key, label, count}], relations: [{from, to, label, count}]}`, the kinds of things in the workspace and the ties between them                                                                                                                                                                                                                                                                                                                                                                                                                  |
| GET `/workspaces/{wid}/map/{kind}?q=&limit=&offset=`                                                                        | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Admins and owners. The things of one kind (`member`, `team`, `project`, `issue`, `graph`, `document`, `agent`) by name: `MapItem[]` = `{kind, id, title, subtitle}`; `limit` 1-50, default 20                                                                                                                                                                                                                                                                                                                                                                                                   |
| GET `/workspaces/{wid}/map/{kind}/{id}`                                                                                     | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Admins and owners. One thing with what it is tied to: `{item: MapItem, ties: {label, kind, count, items: MapItem[]}[]}`. A tie gives its count and its first 12 items; ties to nothing are left out; `run`, `node`, `memory` and `passage` are only counted, `label`, `cycle` and `topic` only named. 404 for a thing the workspace does not have                                                                                                                                                                                                                                               |
| GET/POST `/workspaces/{wid}/transfers` | POST `{url, redis_url?}` (`postgres://…`; `redis://…`/`rediss://…`; used once, not stored) | Owners only. POST 202 `Transfer` and moves the workspace to that database in the background: every row (schema prepared with this build's migrations; one target transaction) plus the workspace's files on disk, carried as `workspace_files` rows that the server owning that database writes out when it next starts; with `redis_url`, the handover is recorded there under `nexc:workspace:<wid>`. GET lists `Transfer[]` latest first: `{id, target, redis_target, status: running\|done\|failed, report: [{table, read, written}], error, created_at, finished_at\|null}`. Password hashes, stored API keys, the `Authorization` headers of MCP servers, the API tokens of external agents and the alert destinations with their delivery log are not copied (they are entered, issued or registered again on the target); 409 while one is running; in production private and loopback targets are refused (422) | done\ | failed, report: [{table, read, written}], error, created_at, finished_at\ | null}`. Password hashes, stored API keys, the `Authorization` headers of MCP servers, the API tokens of external agents and the alert destinations with their delivery log are not copied (they are entered, issued or registered again on the target); 409 while one is running; in production private and loopback targets are refused (422) |
| POST `/workspaces/{wid}/transfers/check` | `{url, redis_url?}` (used once, not stored) | Owners only. "Test connection": 200 `{database_ok, database, redis_ok\|null, redis\|null}` — the database must be reachable and empty of this workspace; Redis is reported only when given (5 s timeout). Nothing is moved |
| GET `/admin/infrastructure`                                                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Platform administrators: `{database: {location, version, size_bytes, pgvector\|null, migrations_applied, migrations_known}, runtime_url, runtime_reachable, runtime_protocol, runtime_grpc: BackendHealth, firewall: {enabled, addr\|null, public_key, open_sessions}, otlp: {enabled, endpoint\|null, reachable, spans_exported, behind, last_exported_at\|null, last_error\|null}, gateway: {calls_per_minute, timeout_secs, max_response_bytes}, embedding_model, ontology: BackendHealth, queue, cache}`                                                                                                                                                                                                                                                                                                        |
| POST `/admin/infrastructure/check`                                                                                          | `{url}` (`postgres://…` or `redis://…`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | `{reachable, detail, pgvector_available\|null, migrations_applied\|null}`. Nothing is changed; the URL is not kept                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| GET `/admin/workspaces?q=&limit=&offset=`                                                                                   | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Platform administrators (`users.role = admin`): `PlatformWorkspace[]` = `{id, name, owner_name\|null, owner_email\|null, member_count, team_count, issue_count, graph_count, created_at}`, newest first. Counts only, never content                                                                                                                                                                                                                                                                                                                                                             |
| GET `/admin/workspaces/{wid}`                                                                                               | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `PlatformWorkspace` + `{members: {user_id, name, email, role, suspended, platform_admin, joined_at}[], footprint: {project_count, document_count, document_bytes, memory_count, run_count}}`                                                                                                                                                                                                                                                                                                                                                                                                    |
| POST `/admin/workspaces/{wid}/owners`                                                                                       | `{email}` of a registered account                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | The workspace detail. Makes the account an owner (joins it if needed) and writes `platform_owner_assigned` to the workspace's audit log. 409 if it already owns it, is suspended or is a platform administrator; 422 for an unknown address                                                                                                                                                                                                                                                                                                                                                     |
| POST `/admin/workspaces/{wid}/delete`                                                                                       | `{confirm}`: the workspace's exact name                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | 204. Removes the workspace with its files; members left without any workspace get an empty personal one. 422 when the name does not match                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| GET `/admin/users?q=&limit=&offset=`                                                                                        | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Platform administrators: `{id, email, name, role: admin\|user, workspace_count, owned_count, locked, suspended, suspended_reason, created_at}[]`                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| PATCH `/admin/users/{uid}`                                                                                                  | `{role?, suspended?, reason?}` (at least one of `role`, `suspended`; `reason` ≤ 300 chars, with `suspended: true`)                                                                                                                                                                                                                                                                                                                                                                                                  | The account. A role change or a suspension ends its access tokens at once; a suspended account gets 403 on login and 401 on refresh. 409 for one's own account, and for making the only owner of a shared workspace an administrator                                                                                                                                                                                                                                                                                                                                                            |
| POST `/admin/users/{uid}/erase`                                                                                             | `{confirm}`: the account's e-mail address                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | 204. Deletes the account exactly as `POST /auth/me/delete` does, on its holder's request. The activity log keeps the account's id (`account_erased`), not its name or address. 409 for one's own account, a platform administrator, or the only owner of a shared workspace; 422 when the address does not match                                                                                                                                                                                                                                                                                |
| POST `/admin/users/{uid}/password-reset`                                                                                    | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `{token, expires_in}` (3600). A one-time password reset token for an account whose holder cannot sign in, shown this once (only its SHA-256 is stored) and replacing any earlier one that still worked. The console turns it into a link, `/reset-password#<token>`, which the administrator hands over: the installation sends no e-mail                                                                                                                                                                                                                                                       |
| GET `/admin/events?q=&limit=&offset=`                                                                                       | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | What platform administrators did, newest first: `{id, action: role_changed\|account_suspended\|account_reactivated\|owner_assigned\|workspace_deleted\|account_erased\|password_reset_issued, actor_id\|null, actor_name, subject, detail, created_at}[]`                                                                                                                                                                                                                                                                                                                                       |
| POST `/workspaces/{wid}/assistant` | `{message, conversation_id?, page?: {path, title}}` | Members. Answers from the workspace's memory and open issues (as the caller sees them) and files issues when asked to, with the caller's rights: 200 `AssistantReply {conversation_id, reply, created: Issue[], skipped: string[], memories_used}`. The exchange is saved to `conversation_id` (one of the caller's, else 404) or to a new conversation named after the message; its last 20 turns are the context. `page` tells the assistant where in the app the caller is. 403 when the workspace's guardrails refuse; 422 for a message over 4,000 characters | `AssistantReply`: answers from the memory and open issues the caller can see; files issues, with the caller's rights, when asked |
| GET `/workspaces/{wid}/assistant/conversations` | — | Members. The caller's own conversations, most recently continued first (at most 200): `Conversation[] {id, workspace_id, title, page_path, page_title, message_count, created_at, updated_at}`. Nobody lists another member's |
| GET/DELETE `/workspaces/{wid}/assistant/conversations/{cid}` | — | Members, own conversations only (else 404). GET `{conversation, messages: [{id, role: user\|assistant, content, outcome: {created: [{id, identifier, title}], skipped, memories_used}\|null, page_path, page_title, created_at}]}` oldest first; DELETE 204 removes it with its messages |
| GET `/workspaces/{wid}/usage?days=`                                                                                         | — (`days` 1–365, default 30)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | `UsageReport`: admins get the whole workspace, other members only their own calls                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| GET `/workspaces/{wid}/invites`, DELETE `/workspaces/{wid}/invites/{iid}`                                                   | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `WorkspaceInvite[]` / 204 (admin+)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| GET/POST `/workspaces/{wid}/teams`                                                                                          | POST `{name, key?, description?, private?}` (member+)                                                                                                                                                                                                                                                                                                                                                                                                                                                               | `Team[]` visible to the caller / 201 `Team`; 409 if the key is taken                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| GET/PATCH/DELETE `/workspaces/{wid}/teams/{tid}`                                                                            | PATCH `{name?, description?, private?}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | `Team` / 204 (team owner or workspace admin)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| GET `/workspaces/{wid}/teams/{tid}/members`                                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `TeamMember[]`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| PUT/DELETE `/workspaces/{wid}/teams/{tid}/members/{uid}`                                                                    | PUT `{role?}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `TeamMember[]` / 204; members may add themselves to a public team and anyone may leave; otherwise team owner or workspace admin                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| GET `/graphs?workspace_id=`                                                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `GraphSummary[]` the caller can open, optionally of one workspace                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| POST `/graphs`                                                                                                              | `{name, description?, goal?, workspace_id?, team_id?}` (default: the caller's first workspace)                                                                                                                                                                                                                                                                                                                                                                                                                      | 201 `Graph`; 403 for a guest outside their teams; 404 for a team the caller is not in                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| GET/PATCH/DELETE `/graphs/{gid}`                                                                                            | PATCH `{name?, description?, goal?}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | `Graph` / 204                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| POST `/graphs/{gid}/nodes`                                                                                                  | `{title, content?, kind?, tags?, x?, y?, executor?, agent_role?}` (`kind` and `executor` default from the ontology)                                                                                                                                                                                                                                                                                                                                                                                                 | 201 `GraphNode`; 422 if `kind` is not a node type of the graph                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| PATCH/DELETE `/graphs/{gid}/nodes/{nid}`                                                                                    | any subset of node fields                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | `GraphNode` / 204                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| PUT `/graphs/{gid}/ontology`                                                                                                | `Ontology` (keys are slugged; missing colours/icons are filled in)                                                                                                                                                                                                                                                                                                                                                                                                                                                  | `Graph`; 422 if a type still in use is removed; 409 if a relation made blocking would close a cycle                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| POST `/graphs/{gid}/edges`                                                                                                  | `{source, target, kind?, reason?}` (`kind` defaults to the first blocking relation)                                                                                                                                                                                                                                                                                                                                                                                                                                 | 201 `GraphEdge`; 422 unknown relation; 409 if a blocking edge would close a cycle                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| PATCH `/graphs/{gid}/edges/{eid}`                                                                                           | `{reason}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | `GraphEdge`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| DELETE `/graphs/{gid}/edges/{eid}`                                                                                          | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 204                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| GET `/graphs/{gid}/suggestions`                                                                                             | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `EdgeSuggestion[]` (dependency detection)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| GET `/graphs/{gid}/analysis`                                                                                                | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `GraphAnalysis`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| POST `/graphs/{gid}/plan`                                                                                                   | `{instructions?}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 202 `Plan` (status `streaming`; progress over SSE)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| GET `/graphs/{gid}/plans/{pid}`                                                                                             | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Plan`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| POST `/graphs/{gid}/plans/{pid}/apply`                                                                                      | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Graph` (plan becomes `applied`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| POST `/graphs/{gid}/runs`                                                                                                   | `{node_ids?: string[], max_concurrency?, force?: boolean}`                                                                                                                                                                                                                                                                                                                                                                                                                                                          | 202 `Run`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| GET `/graphs/{gid}/runs`                                                                                                    | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Run[]` (newest first)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| GET `/runs/{rid}`                                                                                                           | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Run`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| POST `/runs/{rid}/cancel`                                                                                                   | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Run`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| GET `/runs/{rid}/trace?after=&limit=`                                                                                       | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `TracePage`: the run's spans in id order (UUID v7, so the order they started), `limit` 1–1000 (default 200); pass `next` as `after` for the following page                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| GET `/runs/{rid}/trace/verify`                                                                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `TraceVerification`: the chain recomputed from the stored spans; `ok` only if every span hashes as stored, every link holds and the hash sealed at the end of the run is the last one, else `broken` names the first span that does not fit                                                                                                                                                                                                                                                                                                                                                     |
| GET `/runs/{rid}/artifacts`                                                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `Artifact[]`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| GET `/runs/{rid}/artifacts.zip`                                                                                             | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | zip of all artifacts                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| GET `/artifacts/{aid}/download`                                                                                             | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | file, `Content-Disposition: attachment` A quarantined artifact answers 403 with the reason until an admin releases it; `/runs/{rid}/artifacts.zip` leaves quarantined files out |
| GET `/templates`                                                                                                            | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `GraphTemplate[]` (built-in starter graphs)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| POST `/graphs/from-template`                                                                                                | `{template_id, name?, topic?}` (`topic` leads the graph goal: `Topic: …`)                                                                                                                                                                                                                                                                                                                                                                                                                                           | 201 `Graph`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| GET `/orchestrator/status`                                                                                                  | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | `OrchestratorStatus`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| POST `/realtime/tickets`                                                                                                    | `{graph_id}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | `{ticket, expires_in}` single-use, 30 s                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| GET `/graphs/{gid}/events?ticket=`                                                                                          | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | SSE stream (§6)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| GET `/graphs/{gid}/ws?ticket=`                                                                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | WebSocket (§7)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| GET `/metrics` (outside `/api/v1`, at `/metrics`)                                                                           | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Prometheus text (admin bearer or `NEXC_METRICS_TOKEN`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |

Executing a graph: nodes run in dependency order (edges of blocking relations). A node's prompt
context is its own title/content, the meaning of its type, the graph goal, the outputs of its upstream
nodes (each with the reason of the edge that makes it an upstream) and retrieved memories. An upstream
output longer than the budget (12 000 characters for an LLM node, 24 000 for an agent node) is not cut
at the budget: blank padding is removed and the passages most relevant to the node are kept, with
`[… N characters omitted …]` where text was left out. A node whose
upstream failed is `skipped`. Unchanged nodes (same content hash + same upstream outputs) are served
from the result cache (`cached: true`) unless `force: true`.

## 6. SSE — `GET /api/v1/graphs/{gid}/events?ticket=...`

Frames: `id: <seq>\nevent: <type>\ndata: <json>\n\n`; `retry: 3000` first; `heartbeat` every 15 s.
The workspace feed of the AgentSecOps console, `GET /api/v1/workspaces/{wid}/secops/events`
(bearer, §5), carries the `trace.span` and `alert` frames below of every graph the member may open.

| event              | data                                                                                                      |
| ------------------ | --------------------------------------------------------------------------------------------------------- |
| `plan.started`     | `{plan_id}`                                                                                               |
| `plan.node`        | `{plan_id, node: ProposedNode}` (one per refined node, streamed as the LLM produces them)                 |
| `plan.edge`        | `{plan_id, edge: ProposedEdge}`                                                                           |
| `plan.ready`       | `{plan: Plan}`                                                                                            |
| `plan.failed`      | `{plan_id, error}`                                                                                        |
| `run.started`      | `{run: Run}`                                                                                              |
| `node.status`      | `{run_id, node_id, status, attempt, error: string\|null, cached: boolean}`; `waiting_approval` while a tool call of the node waits for a human, `running` again once it was decided |
| `node.output`      | `{run_id, node_id, delta}` (streamed text)                                                                |
| `node.log`         | `{run_id, node_id, level, message}`                                                                       |
| `node.tokens`      | `{run_id, node_id, tokens_in, tokens_out}` — cumulative totals for that node in this run (not increments) |
| `artifact.created` | `{artifact: Artifact}`                                                                                    |
| `trace.span`       | `{span: Span}`, once per span appended to a run's chain, in chain order (`seq`)                           |
| `alert`            | `{alert: Alert}`, once per alert raised about a run of the graph (a DLP finding, a risk score over its threshold, an injection) |
| `run.finished`     | `{run: Run}`                                                                                              |
| `heartbeat`        | `{at}`                                                                                                    |

The audit's answer (`POST /workspaces/{wid}/audit/ask/stream`, §5) streams with the same framing
and the events `stage`, `delta`, `done` and `error`; it is a POST, so clients read it with `fetch`.

## 7. WebSocket — `GET /api/v1/graphs/{gid}/ws?ticket=...` (JSON text frames)

Client → server: `{type:"node.move", node_id, x, y}` (persisted, throttled), `{type:"presence", cursor:{x,y}|null}`, `{type:"ping"}`.
Server → client: `{type:"node.upserted", node}`, `{type:"node.deleted", node_id}`, `{type:"edge.upserted", edge}`,
`{type:"edge.deleted", edge_id}`, `{type:"graph.updated", graph: GraphSummary}`, `{type:"ontology.updated", ontology: Ontology}`, `{type:"suggestions", items: EdgeSuggestion[]}`,
`{type:"presence", user_id, name, cursor}`, `{type:"pong"}`.
Every REST mutation of nodes/edges is broadcast to all sockets of that graph; dependency detection
runs after each node create/update and pushes `suggestions` plus any auto-created `[[wikilink]]` edges.

## 8. Agent runtime internal API (backend → runtime only)

Auth: `Authorization: Bearer $NEXC_RUNTIME_TOKEN` (constant-time compare). Bind to the internal network only.

- `GET /healthz` → `{status:"ok", version, agents_loaded}`
- `POST /v1/execute` → `application/x-ndjson` stream; `POST /v1/approvals/{id}` → the backend hands a
waiting call its decision (below). Request of `/v1/execute`:

```json
{
  "run_id": "…",
  "node_id": "…",
  "token": "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.….…",
  "agent": {
    "name": "researcher",
    "role": "researcher",
    "principal": "agent://0199…/Researcher",
    "system_prompt": "…",
    "model": "claude-opus-5",
    "budget_tokens": 200000
  },
  "task": {
    "title": "…",
    "content": "…",
    "kind": "document",
    "kind_description": "A written deliverable.",
    "produces_artifact": true
  },
  "context": {
    "goal": "…",
    "upstream": [{ "node_id": "…", "title": "…", "output": "…" }],
    "memories": ["…"],
    "documents": ["[report.pdf, p. 17]\n…"],
    "labels": ["internal"]
  },
  "llm": {
    "provider": "anthropic",
    "api_key": "sk-…",
    "model": "claude-opus-5",
    "base_url": null
  },
  "limits": { "max_turns": 12, "timeout_s": 600, "allow_code_exec": false }
}
```

**The per-run agent token.** `token` is an HS256 JWT the backend signs with `NEXC_AGENT_TOKEN_KEY`
(derived from `NEXC_RUNTIME_TOKEN` as SHA-256(`nexc-agent-token-v1:` + token) when unset; the
master key never leaves the backend), issuer `nexc-engine`, audience `nexc-runtime`, claims
`sub` (the principal), `jti`, `iat`, `nbf`, `exp` (the node's `timeout_s` plus 60 s), `run_id`,
`node_id`, `agent_id`, `workspace_id`, `depth` (0 for the root agent), `policy_version`,
`capabilities` (the agent's `Capabilities` document narrowed by every policy level, §4), `rules`
(the resolved `Rules`, §4: deny-lists, approvals, limits, DLP, the injection screen and the
policy versions in force) and `labels` (the classifications of the data the node was given;
`context.labels` says the same to the runtime). A token minted before these claims existed reads
as `Rules` with nothing set and no labels. The runtime starts nothing without a
token that verifies (signature compared in constant time, algorithm `HS256` only, audience, `exp`
and `nbf` with 5 s of leeway, `run_id` and `node_id` equal to the request's): a missing, forged,
expired or misdirected token ends the stream with
`{"type":"error","message":"agent token refused: …","retryable":false}`. Every tool call is
decided against `capabilities.tools` in `Tool.invoke` and every `spawn_subagent` against
`delegation` and `counterparts`; a refused one never runs: it is reported as a `policy` span
(below) and the model gets a tool error naming the rule. A sub-agent is born with a token the
runtime signs for it: the same run and node, `depth + 1`, `sub` = `{parent principal}/{role}-{n}`,
and capabilities narrowed by the parent's (never wider). A model outside `capabilities.models`
is refused by the backend before dispatch and by the runtime at start.

Stream lines (one JSON object per line):
`{"type":"log","level":"info","message":"…"}` · `{"type":"delta","text":"…"}` ·
`{"type":"tokens","input":123,"output":45}` · `{"type":"spawn","agent":{"name":"…","role":"…"}}` · `{"type":"span", …}` (below) ·
`{"type":"artifact","path":"report.docx","mime":"application/vnd.openxmlformats-officedocument.wordprocessingml.document","content_b64":"…"}` ·
`{"type":"result","output":"…","tokens_in":0,"tokens_out":0}` · `{"type":"error","message":"…","retryable":true}`.
The stream always ends with exactly one `result` or `error`. Artifact paths are relative, no `..`, ≤ 20 MiB each.

**Spans.** Every action of an agent is also reported as a span line, emitted when the action ends:

```json
{
  "type": "span",
  "id": "0199f3a0-6b1e-7c2d-9e4f-1a2b3c4d5e6f",
  "parent_id": "0199f3a0-6a00-7000-8000-000000000000",
  "kind": "tool_call",
  "name": "write_file",
  "agent": "writer/researcher-1",
  "started_at": "2026-10-17T12:00:00.123456789Z",
  "ended_at": "2026-10-17T12:00:00.125000000Z",
  "status": "ok",
  "attributes": {
    "tool": "write_file",
    "args_sha256": "…",
    "args_bytes": 812,
    "result_bytes": 24,
    "is_error": false,
    "duration_ns": 1876543
  }
}
```

| `kind`      | emitted for                                                          | `name`     | `parent_id`                          | `attributes`                                                                          |
| ----------- | -------------------------------------------------------------------- | ---------- | ------------------------------------ | ------------------------------------------------------------------------------------- |
| `llm_call`  | every model turn of every agent; a failed call has `status: "error"` | the model  | the agent's `spawn` span (root: null) | `provider, model, turn, tokens_in, tokens_out, tokens_cached, stop_reason, tool_calls` |
| `tool_call` | every tool call that runs (`Tool.invoke`), `spawn_subagent` included | the tool   | the turn that asked for it           | `tool, args_sha256, args_bytes, result_bytes, is_error, duration_ns`                  |
| `spawn`     | a sub-agent's birth, an instant (`started_at == ended_at`)           | the child  | the parent agent's current turn      | `child, role, depth, budget_tokens, policy_version`                                   |
| `policy`    | a tool call or spawn the token refused, an instant; `status: "error"` | the subject (the tool, the role) | the turn that asked | `decision: "deny", rule, reason, action: tool_call\|spawn, subject, policy_version` (+ `depth, children` for a spawn) |

Every `policy` span of the backend also carries `policy_versions` (`PolicyVersions`, §4), as does
the `data_read` span of a node (with `labels`, what the node was given).

**DLP on the way out.** Every artifact a node delivers is stored and then screened: its text
(plain text as it is, the words of a `.docx`, `.xlsx` or `.pptx`; other binaries are not read) is
classified for credentials (the guardrails' scrubber), personal data (`rules.dlp.pii.kinds`:
e-mail addresses, phone numbers, card numbers passing Luhn, national-id shapes) and the
workspace's keywords, and the strictest label the node read (its own context and its upstream
nodes') is checked against `rules.dlp.labels[label].artifacts`. A label that may not leave, a
credential under `secrets: deny` or personal data under `pii.action: deny` keeps the artifact in
(`quarantined_at`, `quarantine_reason`), with a `policy` span of status `error` (`action:
"artifact"`, `artifact_id`, `label`, `secrets`, `pii`, `keywords`, `findings`), an `alert` and an
`artifact_quarantined` audit entry; under `alert` the same span has decision `alert` and only the
alert is raised. A tool that calls out reports `egress_host` (and `payload_sha256`,
`payload_bytes`) on its `tool_call` span; the backend checks the host against the egress rules
(`egress.deny`, then the allow-list) and the node's labels against
`rules.dlp.labels[label].egress_hosts` (`action: "egress"`), and a denial is a `policy` span and
an alert: the call already happened, so what left is on the record. Spans and alerts carry
counts and digests, never the content.

**Behaviour baseline and risk.** When a run finishes, each agent that worked in it is scored
against its baseline before the baseline learns the run: tool calls per turn, tokens per turn,
distinct tools, distinct hosts, sub-agents, policy denials (and mismatches) and bytes out
(artifact sizes plus `payload_bytes` of calls out) as running means and deviations (Welford), the
MinHash of the run's tool sequence against the last twenty runs. Features a baseline of at least
three runs has seen score their z (capped at 3 each), hosts and tools never seen before add to it,
and a sequence unlike every earlier run adds one; the score is capped at 10 and every unusual
feature is a reason in words (`Agent.risk_score`, `risk_reasons`). The policy's `alerts` turn it
into alerts (`alerts.risk_score`, `alerts.denials_per_run`, `alerts.novelty`, kind `risk`) and,
at `alerts.quarantine_risk_score`, into a quarantine (as `POST /agents/{id}/quarantine` does,
audited as `agent_quarantined` with `automatic: risk score …`, a `critical` alert). The run's
trace is sealed by then, so these alerts name no span.

**The prompt-injection screen.** Data about to go to the model is looked at for instructions
aimed at it, cheaply and without a model: a fixed set of patterns (ignoring earlier instructions,
new or real instructions, role overrides, prompt-leak requests, exfiltration of keys and secrets,
tool hijacking, chat-template markup, directives to the assistant, urgency overrides) and
zero-width or tag characters that hide text from a reader; two patterns together, markup, hidden
text or exfiltration rate `high`. The backend screens every upstream output, memory and document
passage as it builds a node's context; the runtime screens every tool result (a sub-agent's answer
included) before it goes to the model. A finding is a `policy` span named `injection` (`rule:
"injection"`, `action: "context"` with `source` `upstream` | `memory` | `document` and `subject`
the node, memory or chunk id, or `action: "tool_result"` with `subject` the tool; `severity`,
`patterns`, `hidden_chars`), decision `alert` by default: the text goes through. Under
`rules.injection.action: "block"` the decision is `deny`, an upstream output or a tool result is
replaced by a notice (`[… withheld by policy: it contained instructions aimed at the model (…)]`)
and a memory or passage is dropped. A `high` finding is also an alert of kind `injection`. With
`rules.injection.model_check` and the runtime setting `RUNTIME_INJECTION_MODEL_CHECK=true`, the
run's model is asked to confirm a tool-result finding first (one short turn, accounted to the
run); a `NO` clears it.

**Approvals.** A tool the rules hold (`require_approval.tools`, or `write_file` / `make_docx`
after data of a label in `require_approval.labels`) does not run at once. The runtime reports a
`policy` span with `decision: "require_approval"`, `rule`, `reason`, `action: "tool_call"`,
`subject` (the tool), `approval_id` (a UUID v7 it chose) and `args_sha256`, logs that it waits,
and keeps the call waiting for `rules.require_approval.timeout_s` (heartbeat log lines every 20 s
keep the stream alive). The backend records the request (`Approval`, §4), sets the node run and
the node to `waiting_approval`, puts an `approval` entry in every approver's inbox and watches the
approvals table; when someone decides it hands the decision to the runtime holding the call,
`POST /v1/approvals/{approval_id}` with the runtime's bearer token and `{"decision": "approved" |
"denied", "reason", "decided_by"}` (200 `{"status":"delivered"}`; 404 when no call waits for that
id on this runtime), records an `approval` span and sets the node back to `running`. Approved, the
tool runs and its `tool_call` span carries `approval_id`; the backend's re-check accepts a held
tool only with an approval it granted. Denied, the model sees `"{tool} was denied by {who}:
{reason}"` as a tool error and goes on. Nobody deciding in time ends the stream with
`{"type":"error","message":"approval for {tool} not given within {n}s","retryable":false}` and
the approval is `expired`. A decision has to reach the runtime instance that holds the call:
with several runtime replicas behind one address, route by run or accept that a misrouted
decision times out (phase 4's stream removes the hop). The deny-lists (`tools_denied`,
`models_denied`) are refused before the allow-lists (`rule` `tools.deny`, `models.deny`), and
`limits.tool_calls_per_minute` refuses the calls past it within a minute (`rule`
`limits.tool_calls_per_minute`; `finish` is never held back). `limits.tokens_per_run` caps the
agent's budget.

`args_sha256` is the SHA-256 of the arguments as JSON with sorted keys and no whitespace. Ids are
UUID v7 made by the runtime; times are RFC 3339 UTC with nanoseconds; `agent` is the acting
principal: the token's `sub` for the root agent, `{parent}/{role}-{n}` for a sub-agent. `llm_call`
and `tool_call` spans also carry `policy_version`. Spans never carry tool arguments, file
contents, prompts or credentials: only digests, sizes, counts and names.

The runtime is not trusted with the trace. The backend reads span lines field by field and
decides the run, the node and the workspace agent itself. It keeps a reported id only when it is
a UUID v7 from about the time of the attempt and not used before; otherwise the span gets a fresh
id, the reported one is kept as `attributes.runtime_id`, and `parent_id`s naming it follow. It
accepts only the four kinds above (`data_read`, `egress` and `approval` are the backend's to
record) and ignores any other kind with a warning. It keeps only scalar attributes
and arrays of scalars (strings ≤ 1024 characters, ≤ 64 attributes), and drops the attributes of a
span line over 64 KiB (`attributes_dropped: true`). Spans are hashed into the run's chain in the
order the backend receives them; lines after the terminal event are never read, and more than
20,000 spans from one attempt are ignored.

**The re-check.** The backend decides every reported span again from the capabilities it minted
for the node: a `tool_call` must be a tool the policy allows, a `spawn` must fit the delegation
depth, the fan-out (counted per principal across the attempt) and the counterpart roles, an
`llm_call`'s `model` must be in the allow-list, a `policy` span's `decision` must be the one the
backend takes for the same action and subject, and `agent` must be the token's principal or a
sub-agent under it (`{principal}/…`; anything else is attributed to the principal with the
reported value kept as `attributes.reported_principal`). Each disagreement is a `policy` span of
the backend's named `policy_mismatch` (`decision: "mismatch"`, `rule`, `reason`, `span_id`,
`principal`), a `policy_mismatch` entry in the workspace's audit log, one more on
`Run.policy_mismatches` and on `nexc_policy_mismatches_total`. The run goes on: a mismatch is an
incident on the record, not a failure of the work.

**The gRPC stream (the default wire, `NEXC_RUNTIME_PROTOCOL=grpc`).** The runtime also serves
`nexc.runtime.v1.AgentRuntime` on `RUNTIME_GRPC_PORT` (8095), the service of
`backend/proto/nexc/runtime/v1/runtime.proto` (vendored once; the runtime's stubs are
regenerated by `make proto-runtime`). `Health` answers `{ok, version, agents_loaded,
runs_active}`. `Execute(stream BackendCommand) returns (stream RuntimeEvent)` is one call per node
attempt: the backend opens it with the bearer token as metadata `authorization`, sends
`ExecuteRequest` as the first command (the fields of the JSON above, plus `gateway_tools`: the
tools the gateway offers this node, each with a JSON Schema) and reads events until the terminal
one. The backend is the client of this stream, not the runtime (the plan wrote the two sides the
other way round): the runtime stays a service the backend reaches on the internal network, with
no route of its own into the backend, and nothing changes for deployment. Events are the NDJSON
lines as messages (`log`, `delta`, `tokens`, `spawn`, `span` with `attributes` as a `Struct`,
`artifact` with `content` as bytes rather than base64, `result`, `error`) plus two the stream
alone carries, `approval_request` and `tool_request`. Commands are `ack` (`request_id`: the
request was taken on), `cancel` (the runtime stops the run and the stream ends with `error`),
`approval_decision` (`{approval_id, decision, reason, decided_by}`: what `POST
/v1/approvals/{id}` carries, but on the stream that holds the call, so a decision never misses
its replica), `policy_update` (`{token}`: a fresh per-run token after the policy changed, used
from the agent's next turn) and `tool_result` (`{request_id, output, is_error}`, the answer to a
`tool_request`). A runtime at `RUNTIME_MAX_CONCURRENT_RUNS` answers `RESOURCE_EXHAUSTED`, which
the scheduler retries; closing the backend's side cancels the run as `cancel` does, and the
backend waits up to 5 s for the runtime's last word. A stream lost mid-run fails the attempt as a
retryable error; an attempt is never resumed. The NDJSON endpoint stays for one release
(`NEXC_RUNTIME_PROTOCOL=ndjson`): it offers no gateway tools and approvals reach it over `POST
/v1/approvals/{id}`.

**The tools gateway.** An agent never reaches the network itself. MCP servers registered for the
workspace (`/workspaces/{wid}/mcp-servers`, §5) and the built-in `fetch_url` are offered to a
node as `gateway_tools` as far as the policy lets it: a server's tools as `mcp:<name>/<tool>`,
when the node's capabilities name the tool or `mcp:<name>/*` and the context's labels do not
exceed the server's `max_label`; `fetch_url` when the capabilities name it. The runtime calls such
a tool by sending `tool_request` `{request_id, tool, arguments, agent, turn_span_id}` up the
stream and waiting for the `tool_result` command; the backend performs the call in this order:
the rate limit (`NEXC_GATEWAY_CALLS_PER_MINUTE` per agent, a token bucket with a burst of a
quarter; rule `limits.gateway_calls_per_minute`), the policy decision on the tool (the deny-list,
the allow-list, `require_approval` as for any tool: the runtime raises the `approval_request`
first and sends the granted id as `__approval_id` in the arguments), the policy decision on the
host (`egress.hosts`: the server's URL host, `local:<name>` for a stdio server, or `fetch_url`'s
URL with every redirect hop checked again; loopback, private and link-local addresses are
refused as `egress.private` unless the allow-list names the host exactly), the DLP screen of the
arguments under the context's labels (as for any egress: `block` refuses, `redact` sends the
redacted text, with the `policy` span of rule `dlp.*`), the timeout
(`NEXC_GATEWAY_TIMEOUT_SECS`) and the response size (`NEXC_GATEWAY_MAX_RESPONSE_BYTES`, cut
with a note). Every call is a `tool_call` span of the backend's (`tool`, `args_sha256`,
`args_bytes`, `result_bytes`, `is_error`, `duration_ns`, `gateway: true`, `server`, `request_id`,
`approval_id`, `policy_version`, `egress_host`, under the turn span the runtime named) and,
when something was reached, an `egress` span under it (`host`, `transport`, `tool`, `server`,
`bytes_out`, `bytes_in`, `status`, `url`); a refusal is a `policy` span naming the rule and the
agent sees the reason as a tool error. A stdio server is started for every call with an empty
environment (PATH, HOME, TMPDIR only) in a scratch directory and killed when the call ends; an
HTTP server is spoken to with MCP streamable HTTP (`initialize`, `tools/list`, `tools/call`,
`mcp-session-id`, SSE answers read), with the registered `Authorization` header. The gateway's
own cost per call, the tool excluded, is what `make prove-gateway` measures.

**The Agent Firewall API (`NEXC_GRPC_ADDR`).** Agents that run elsewhere use nexc as their policy
and trace service through `nexc.firewall.v1.AgentFirewall`
(`backend/proto/nexc/firewall/v1/firewall.proto`), served with reflection and `grpc.health.v1`
where `NEXC_GRPC_ADDR` points and off when it is unset. The caller is a workspace agent of
runtime `external` holding an API token from `POST /agents/{id}/tokens`, sent as metadata
`authorization: Bearer nexc_agt_…`; the token and the agent's status are read on every call
(`UNAUTHENTICATED` for an unknown or revoked token, `PERMISSION_DENIED` for a quarantined or
paused agent) and the resolved policy is kept for 5 s per policy version. `AuthorizeAction
{principal, action, subject, labels, context}` (`action` one of `tool_call`, `egress`, `model`,
`spawn`, `data_read`; `principal` the agent or a sub-agent under it) decides as the backend
decides for its own agents and answers `{decision, rule, reason, policy_versions, span_id,
run_id, decided_at}` signed: `signed` is the canonical JSON of those fields with the principal,
action, subject and labels, `signature` its Ed25519 signature and `public_key` the key
(`NEXC_FIREWALL_SIGNING_KEY`, shown on `/admin/infrastructure`), so whoever holds the key can
check a decision later. Every decision is a `policy` span on the agent's open run. `ReportSpan`
(client streaming) takes `start {external_run_id, title}`, any number of `span`s and `finish
{status, output}` and answers `{run_id, spans, trace_hash, policy_mismatches}`: the spans go into
the chained trace exactly as a runtime's would (ids checked, kinds limited, attributes bounded,
every span re-decided and each disagreement a `policy_mismatch`), in a run of the agent's own
managed graph (`graphs.external_agent_id`, one node per external run). `VerifyTrace {run_id}`
recomputes the chain as `GET /runs/{rid}/trace/verify` does; `GetAgentCard` answers the caller's
own card. A run with no call for 10 minutes is sealed and finished.

**The OpenTelemetry export (`NEXC_OTLP_ENDPOINT`).** When set, a worker sends every span of the
trace to the collector over OTLP/gRPC as it is written, in batches of up to 1000 behind one
cursor (`otlp_export_cursor`, taken with `SKIP LOCKED` so instances share it; at least once). The
run is the trace (its id the run's UUID), a span's id the last 8 bytes of its UUID v7, its
parent the `parent_id`, the workspace the resource (`service.name: nexc-engine`,
`nexc.workspace_id`, `nexc.workspace_name`), the scope `nexc.trace`; the span's attributes go as
they are stored, with `nexc.kind`, `nexc.principal`, `nexc.run_id`, `nexc.node_id`,
`nexc.agent_id`, `nexc.seq` and `nexc.hash` beside them. Nothing that is not in a span leaves:
digests, names, hosts and counts, never arguments, prompts or content. `nexc doctor` and
`/admin/infrastructure` report whether the collector answers, how many spans went and how far
the cursor is behind.

## 9. texc-symphony bridge

When `NEXC_SYMPHONY_ENABLED=true`, nodes with `executor: "symphony"` are written as issues
(`identifier: NEXC-<last 8 hex of node id>` (UUID v7 prefixes are timestamps and collide), `state: Todo`) into the managed `WORKFLOW.md`
(`tracker.kind: memory`), then the backend calls `POST {SYMPHONY_URL}/api/v1/refresh` and follows
`GET /api/v1/runs` (every 500 ms) until the issue's run ends.

The bridge owns a directory, the parent of `NEXC_SYMPHONY_WORKFLOW`, which the backend and Symphony
must both see at the same path:

| Path                      | Content                                                                     |
| ------------------------- | --------------------------------------------------------------------------- |
| `WORKFLOW.md`             | the managed workflow                                                        |
| `repos/<graph id>.git`    | one bare git repository per graph; `main` starts with a `.gitignore` commit |
| `issues/<identifier>`     | path of the repository the issue's workspace is cloned from                 |
| `workspaces/<identifier>` | Symphony's issue workspaces (`workspace.root`)                              |

The workflow sets `agent.max_turns: 1`, `agent.max_concurrent_agents: NEXC_MAX_CONCURRENCY`,
`agent.continuation_delay_ms: 15000` (texc-symphony waits that long before running a still-open
issue again; older versions ignore the key and may run a node twice), an `after_create` hook that
clones the graph repository into the workspace and, when `NEXC_SYMPHONY_AGENT_COMMAND` is set,
`codex.command` with `approval_policy: never`.

When a run succeeds the backend commits the workspace, rebases it on the repository's `main` and
pushes (retrying when a parallel node pushed first), then moves the issue to `Done`, which makes
Symphony delete the workspace. Nodes that run later therefore start from the code of the nodes
before them. A change that cannot be rebased fails the attempt as retryable and the next attempt
starts from the newer `main`; a failed or cancelled run moves the issue to `Cancelled`. The node
output is the agent's last message followed by the merged commit and its files; tokens come from
the run record.

`scripts/symphony-claude-agent.py` is an agent command that runs each turn with the Claude Code
CLI (`claude -p`) instead of Codex.
