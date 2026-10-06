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
- Knowledge base: a workspace uploads documents (PDF, Word, Excel, PowerPoint, HTML, CSV, Markdown,
  text; Settings -> Knowledge). The agent runtime parses them (`POST /v1/parse`), they are split
  into heading-aware passages (table rows carry their multi-row headers), embedded through an
  OpenAI-compatible endpoint (`NEXC_EMBED_*`, or per workspace; a built-in word-matching embedding
  otherwise) and searched by keywords and embedding similarity
  (`GET /workspaces/{wid}/knowledge/search`). Running nodes and the planner get the passages that
  bear on their task, with citations, within a per-workspace passage count and character budget.
- Document search by meaning: with pgvector, passages get an HNSW index per embedding size and a
  search also takes the query's nearest neighbours, so a passage is found even when it shares no
  word with the question (a neighbour less than 0.3 similar is not an answer).
- Topics: a workspace's passages are clustered by their embeddings and each cluster is named by
  the words that set it apart (`GET /workspaces/{wid}/knowledge/topics`); a search can be kept to
  one topic (`topic_id`), and admins can find the topics afresh.
- `deploy/postgres/pgvector.Dockerfile` builds PostgreSQL 17 with pgvector; `scripts/db.sh` now
  reads `NEXC_DB_IMAGE` and `NEXC_DB_VOLUME` from `.env`.
- Memory topics: a workspace's memories are grouped into topics the same way documents are
  (`GET /memories/topics`, `topic_id` on `GET /memories`), shown as chips on the Memory page.
  Topic names are drawn only from memories the whole workspace can read; personal notes and
  private teams' memories are placed under a topic but never shape its name.
- Memory can forget, when a workspace says so: guardrails gain `memory_limit` and
  `memory_forget_after_days` (both off by default). Hourly, memories neither recalled nor updated
  for that long are removed, then the least important and least recalled beyond the limit.
- Embedding calls are booked in the usage ledger under the purpose `embedding` (tokens; the price
  of embedding models is not known to the server, so their cost is recorded as zero).
- Activity: what happened in a workspace on a day, across the audit log, issues, graphs, runs,
  documents and memory (`GET /workspaces/{wid}/timeline`, `…/timeline/days`), with a 30-day strip
  of busy days. Workspace map: the kinds of things in a workspace and the ties between them, with
  counts (`GET /workspaces/{wid}/map`). Both for admins and owners.
- Day summaries: on the Activity page an admin can have the workspace's AI tell a day in a few
  lines (a headline, what happened, what deserves a look). The summary is kept, so reading it
  again spends nothing, and is marked when more has happened since. The day reaches the model
  as a digest of bounded size, the call obeys the workspace's guardrails and is booked as
  `summary` usage, and summaries travel with a workspace transfer
  (`GET`/`POST /workspaces/{wid}/timeline/summary`). The demo provider answers with the counts.
- Workspace map, one thing at a time: pick a member, team, project, issue, graph, document or
  agent and see exactly what it is tied to (a member's teams, issues and graphs; an issue's
  team, project, people, sub-issues, graph, labels and cycle), then step on to any of those.
  The trail of steps stays on screen (`GET /workspaces/{wid}/map/{kind}`, `…/map/{kind}/{id}`).
- A test fails when a table that belongs to a workspace is neither copied by a transfer nor
  listed as an exception with its reason, so data added later cannot be left behind unnoticed.
- Infrastructure page for server administrators: database, pgvector, migrations, agent runtime,
  embedding model, and a check that another PostgreSQL or Redis is reachable
  (`/admin/infrastructure`). `docs/INFRASTRUCTURE.md` explains what can be changed and how.
- Data transfer: a workspace's owner copies the whole workspace to a PostgreSQL they control
  (`POST /workspaces/{wid}/transfers`, Settings -> Data transfer) and can then remove it from this
  server. Password hashes and stored API keys never leave; the connection string is not kept.
  Deleting a workspace now also deletes the uploaded files of its documents.
- Deleting a graph or a workspace removes the artifact files of its runs from disk, not only
  their rows.
- A workspace's files on disk (uploaded originals, run artifacts) download as one archive in the
  layout of the data folder (`GET /workspaces/{wid}/files.zip`), completing a data transfer.
- Platform console (`/app/platform`): what a platform administrator works in, and all such an
  account sees. It has the app's layout with its own menu (Workspaces, Accounts, Infrastructure):
  every workspace with its owner and size, every account with its platform role (which another
  administrator can change). The workspace app and its settings send a platform administrator
  there; everyone else is sent back to their workspace (`/admin/workspaces`, `/admin/users`).
- Platform console actions: open a workspace to see its members and how much it holds, assign an
  owner to a workspace whose owner left or cannot sign in (recorded in that workspace's audit
  log), and delete a workspace after typing its name; suspend and reactivate accounts with a
  reason. Everything done from the console is kept in its Activity log (`/admin/workspaces/{wid}`,
  `/admin/workspaces/{wid}/owners`, `/admin/workspaces/{wid}/delete`, `/admin/events`).
- Suspended accounts: sign-in answers 403 once the password was right, refresh is refused, and
  the access token already in hand stops working at once. The Members page of every workspace
  the account is in marks it as suspended (`WorkspaceMember.suspended`).
- `make seed-demo` (`scripts/seed-demo.py`): a demo workspace with five accounts in different
  roles, three teams, projects, cycles, issues and comments, created through the API so that
  each account's inbox holds real notifications.
- Workflow editor: the Teams page opens a team's workflow, where team owners and workspace admins
  add, rename, recolour, reorder and remove issue states; other members see it read-only.
- "Your teams" in the sidebar opens a team's issues (`/app/issues?team=<id>`), and `C` on the
  Issues page starts a new issue.

### Fixed

- The API description referenced a schema it did not define (a path parameter's type), which
  broke generating the client types; a test now checks that every reference resolves.

### Changed

- The platform and the workspaces are separate APIs. A platform administrator's token is refused
  (403) on every workspace route, whatever the account is a member of, and is accepted on
  `/auth/*` and `/admin/*` only; before, the separation was in the interface alone. A platform
  administrator created by the server or the command line no longer gets a personal workspace,
  and the only owner of a workspace other people work in cannot be made one until the workspace
  has another owner.
- Changing an account's platform role takes hold at once instead of at the next token refresh:
  access tokens carry a session epoch (`epoch` claim) that the change raises.
- `PATCH /admin/users/{uid}` takes `suspended` and `reason` next to `role`, all optional.
- A workspace transfer copies every referenced account as an ordinary, active user: nobody
  administers, or is suspended on, another installation because they were here.
- Nodes run by the agent runtime get the workspace's document passages in a section of their
  own (`context.documents`, rendered as `<document source="…">`), with their own limits and a
  citation rule, instead of mixed into the memories where they competed for the same twenty
  places.
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
- Settings are grouped by what they are about (Account, the workspace's people, AI, Insight,
  Data); what concerns the whole installation moved to the platform console. The assistant opens
  from a notch on the right edge of the screen instead of a round button in the corner.
- Without a remembered choice the app opens the workspace with the most members, so someone
  invited to a team's workspace lands there and not in their own empty personal one.
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
