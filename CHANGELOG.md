# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Per-graph ontology: node types and relation types are data owned by each graph instead of fixed
  enums. `GET /graphs/{gid}` returns it, `PUT /graphs/{gid}/ontology` replaces it, and the canvas
  toolbar has an editor for it.
- Edges carry a `reason` (`PATCH /graphs/{gid}/edges/{eid}`), shown in the new edge inspector and
  passed to downstream nodes with the upstream output.
- The planner proposes new node and relation types for the goal's domain and justifies every edge.
- Workspaces, members and teams with Linear-style roles (workspace owner / admin / member /
  guest, team owner / member, private teams), invitations by e-mail that are accepted on sign-up,
  and a personal workspace for every account.
- Graphs belong to a workspace and optionally a team (`workspace_id`, `team_id`); access to a
  graph, its runs and artifacts follows membership instead of who created it. The sidebar has a
  workspace switcher and lists are scoped to the open workspace.
- Workspace credentials: admins set one LLM credential per workspace (`/workspaces/{wid}/llm`);
  a member's own connected account takes precedence, then the workspace's, then the server
  default. `DELETE /settings/llm` disconnects a personal account. `LlmSettings` gains `scope`.
- `GET /settings/llm/models` lists the models the effective provider offers, asked from the
  provider at request time (Anthropic and OpenAI-compatible endpoints).
- Members and Teams pages, and a settings page split into "Your AI account" and "Workspace
  credential".
- Issue board: one column per state of a team's workflow, drag a card to change its state; the
  list/board choice is remembered.
- List queries are kept in the browser between visits, per account, and removed on sign-out, so
  pages paint immediately and refresh in the background.
- Toasts no longer cover the assistant button.
- Guardrails per workspace: monthly token budgets (workspace-wide and per member), an allow-list
  of providers, a switch for agent code execution, and removal of API keys, tokens and private
  keys from the context sent to models (on by default). Checked before plans, runs and assistant
  replies; a card in Settings edits them.
- Workspace assistant: a chat in the corner of every page that answers from the workspace's
  memory and open issues and files issues when asked, with the member's rights.
- Memory search uses pgvector when the database has it: an `embedding_vec vector(256)` column
  with an HNSW index is created at startup, and workspaces with 5 000 or more memories are
  narrowed by nearest neighbours plus full-text matches before ranking. Without the extension
  nothing changes. `NEXC_DB_IMAGE` selects the dev database image.
- Issues and projects: every team has a workflow whose states are data (starter: Backlog, Todo,
  In Progress, In Review, Done, Canceled), issues are numbered per team (`ENG-12`) with
  priority, assignee and project, and `POST /issues/{iid}/graph` turns an issue into a graph
  that plans and executes it. Issues and Projects pages.
- Upstream context is fitted to the node instead of cut after a fixed length: padding is removed
  and, when an upstream output is still too long, the passages most relevant to the node's task
  are kept. The characters left out are recorded per call and shown on the Usage page.
- The agent runtime turns on Anthropic prompt caching for its tool loop, so each turn reads the
  earlier turns from cache.
- Usage ledger: every LLM call that spends tokens (planning, node execution, memory extraction)
  is recorded with the member who caused it, the model and whose account paid.
  `GET /workspaces/{wid}/usage?days=` reports totals and breakdowns by day, member, model,
  purpose and paying account (admins see everyone, members their own), and there is a Usage page.
- Agents belong to a workspace: every workspace gets the default organisation, members share
  and edit its agents, and nodes are assigned to agents of the graph's workspace.
- Memory belongs to a workspace: what a graph learned is readable by everyone who can open that
  graph, and work on one graph also recalls what the workspace's other graphs learned (weighted
  below the graph's own memories). Private-team graphs keep their memory to the team.
- Memory retrieval reads from an in-process index (one `DashMap` entry per workspace, 30 s TTL,
  dropped on write) instead of querying PostgreSQL per node, ranks every memory in scope, and
  records accesses in the background.
- The agent runtime stops the `claude` CLI when a node is cancelled or times out.
- Canvas legend with a switch for dependency suggestions (now off by default).
- Issue timeline: comments (`POST /issues/{iid}/comments`, edited by their author, deleted by the
  author or whoever manages the team) and a history of changes to state, priority, assignee and
  title (`GET /issues/{iid}/events`), shown under the issue.
- Labels: a workspace's labels (`/workspaces/{wid}/labels`) go on issues (`label_ids`, returned as
  `labels`), show as chips in the list and on the board, are picked or created from the issue, and
  filter the list (`label_id`).
- Audit log: changes to a workspace's members, invitations, teams, credential, guardrails and
  labels are recorded with who made them (`GET /workspaces/{wid}/audit`, admins and owners) and
  listed on a new Audit log page.
- Inbox: members are told when an issue is assigned to them and when someone else comments on or
  moves an issue they created or are assigned (`GET /workspaces/{wid}/inbox`, `POST …/inbox/read`).
  The sidebar shows the unread count, and an issue opens by address (`/app/issues?issue=<id>`).
- Sub-issues: an issue can be part of another (`parent_id`); the parent shows its parts with how
  many are closed (`sub_issues`), parts are added from the issue, and `parent_id` filters the list.
- Cycles: teams plan non-overlapping, numbered time boxes (`/workspaces/{wid}/teams/{tid}/cycles`)
  from the Teams page; issues are planned in one (`cycle_id`), cycles show how many of their issues
  are closed, and a team's issue list filters by cycle.
- Project page (`/app/projects/<id>`): a project's name, summary, status, lead, target date and
  progress, all saved as they are changed, over the project's own issues as a list or board. New
  issues can be filed straight into a project.
- Workflow editor: the Teams page opens a team's workflow, where team owners and workspace admins
  add, rename, recolour, reorder and remove issue states; other members see it read-only.
- "Your teams" in the sidebar opens a team's issues (`/app/issues?team=<id>`), and `C` on the
  Issues page starts a new issue.

### Changed

- `kind` on nodes and edges is a key of the graph's ontology; edges gain `blocking`, which replaces
  the special meaning of `depends_on` in scheduling, cycle checks and analysis.
- Node type attributes are part of the result-cache hash, so cached node results from earlier
  versions are not reused.
- The logged-in shell uses an inset sidebar and a neutral colour theme; the window no longer
  scrolls on the graph page.
- Settings are their own screen with their own sidebar (`/app/settings/…`): Profile and
  Preferences for the account; Members, Teams, AI accounts, Guardrails, Usage and Audit log for
  the workspace. The old addresses (`/app/teams`, `/app/members`, `/app/usage`, `/app/audit`)
  redirect there, and the app sidebar keeps a single Settings link.
- The app sidebar uses the default layout and stays open on desktop; it no longer collapses to
  icons.
- Memory moved into settings (`/app/settings/memory`) and shows ten memories a page as one-line
  previews; opening one reads it in full (`GET /memories/{id}`, and `offset`/`preview` on the
  list). An opened issue is likewise read again on its own instead of shown from the list.
- A run reads the outputs and titles of a node's upstream nodes in one query each instead of one
  per upstream node.
- Memory no longer stops at a workspace's 20,000 most recent memories. A workspace with 5,000 or
  more is searched in the database (full-text matches of the query's rarest words, the most
  recent memories, and nearest neighbours when pgvector is present) and ranked from a few hundred
  candidates; the memory list and single memories are always read from the database. Only small
  workspaces are held in process.
- Linear-style interface pass: a denser type scale, thinner icons, a sidebar grouped into Work,
  Your teams, Build and Workspace, and issue rows and board cards that show state and priority as
  glyphs instead of labels.
- The issue opens as a two-column view: title, description, sub-issues and activity on the left,
  status, priority, assignee, labels, project and cycle on the right. Title and description save
  when the field is left; the Save button is gone. Projects are listed as rows with status, lead,
  target date and progress.

## [0.1.0] - 2026-10-04

First public release.

### Added

- Visual graph editor with typed nodes, `[[wikilink]]` auto-edges, dependency suggestions and live
  collaboration over WebSocket.
- LLM plan refinement streamed over SSE, reviewed and applied atomically.
- DAG scheduler with concurrency limits, retries, timeouts, cancellation, critical-path analysis
  and a content-hash result cache.
- Executors: direct LLM calls, the Python agent runtime and the optional texc-symphony bridge.
- Python agent runtime: agents born from a spec per request with role personas, token budgets,
  sub-agents (depth 2, up to 4 children), workspace-confined tools, `.docx` generation and an
  opt-in sandboxed `run_python`.
- LLM providers: Anthropic (default `claude-opus-5`, adaptive thinking, server-side refusal
  fallbacks), the local Claude Code CLI with its own login (`claude_code`, no API key),
  OpenAI-compatible endpoints, and an offline deterministic `demo` provider.
- Six starter templates that ask for a topic; document and output nodes always deliver a `.docx`.
- Mem0-style memory (extract, consolidate, hybrid retrieval: embeddings + BM25 + recency);
  C11 kernels for hashing, MinHash, embeddings and SHA-256.
- Authentication with Argon2, rotating refresh tokens with reuse detection, CSRF guard and
  AES-256-GCM encrypted API keys.
- PostgreSQL 17 storage with realtime fan-out over `LISTEN/NOTIFY`.
- OpenAPI document and Scalar API reference; generated frontend types.
- Docker images (non-root, read-only), Docker Compose stack with a demo overlay, Kubernetes
  manifests with NetworkPolicies, Makefile workflows, CI, CodeQL and release automation.

[Unreleased]: https://github.com/f8fvwgzc/nexc-engine/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/f8fvwgzc/nexc-engine/releases/tag/v0.1.0
