# nexc-engine as a product on four cores: the migration plan

_Written 2026-10-26 against `main` at `c259cb6` (after Phase 0's #19 merged), with the security
PRs #20 (audit blockers) and #21 (gRPC TLS) open. This is a plan. It moves no code._

The owner decided on 2026-10-26: nexc-engine becomes a **product**, a Linear-style project tool
(issues, projects, cycles, teams, boards, notifications). It is built on four core repositories,
each the single owner of one concern. This document says, for every module of `backend/src/` and
for `agent-runtime/`, whether it **stays** in the product or **moves** to a core. It then says in
what order the modules move, how their data migrates, and which tests go with them. It also maps
today's LLM settings onto the platform's LLM connection contract v1.

It lines up with Tutela's own plan for this repo, `docs/INTEGRATION.md` §5 in
[f8fvwgzc/tutela](https://github.com/f8fvwgzc/tutela). Where the two disagree, Tutela's document
decides the Tutela side, and this one is corrected.

---

## 1. The product and the cores

| Repo | Kind | Owns after the migration |
| --- | --- | --- |
| **nexc-engine** | product | Issues, projects, cycles, labels, issue states, teams as a product object (key, name, workflow), boards, the inbox and notifications, the issue assistant, day summaries, plans of work (graphs as a breakdown of an issue), the console UI, and realtime fan-out to browsers. |
| **Tutela** (`f8fvwgzc/tutela`) | core | Users, orgs, memberships, groups, roles, field and UI permissions, API keys (`tut_sk_…`, `tut_ak_…`), sign-in through the OIDC issuer, agent token exchange (RFC 8693), the secret vault (`tutela-vault`), the SSRF-safe HTTP client (`tutela-safenet`). |
| **attache** (`f8fvwgzc/agent-sidecar`) | core | AgentSecOps: the agent registry, capabilities and policies, the firewall, approvals, risk and quarantine, DLP, the evidence ledger (hash-chained trace, seals, bundles), the agent runtime, the tool gateway (MCP, `fetch_url`), A2A, and the model gateway that meters and limits every model call. |
| **onto-intelli** | core | The ontology (already there today), knowledge (documents, passages, embeddings, retrieval), and agent memory. |
| **texc-symphony** | core | Executing tasks on any agent backend and any LLM, through its task API (create, list, get, cancel, unblock, an SSE watch) and the `symphony-tasks-client` crate (texc-symphony PR #5). |

The product calls each core through that core's published client (`tutela-sdk`,
`symphony-tasks-client`, onto-intelli's gRPC API, attache's APIs). It never uses a path
dependency, a sibling checkout or a copied file.

## 2. Rules every move follows

1. **The destination first.** Each move lands in the core's repository first, in that core's own
   PRs. Only then does nexc-engine switch to it. A move is never a copy of nexc code into a core
   by this repository.
2. **Shadow, flip, remove.** Every decision a core takes over (a role check, a firewall decision,
   a retrieval) runs in three stages, as in Tutela's `INTEGRATION.md` §1:
   - **Shadow:** nexc's old code decides, the core's answer is only logged (opaque ids only,
     never tokens, e-mails or names). This runs for **1–2 weeks of logged decisions**, until every
     mismatch is explained and fixed.
   - **Flip:** the core decides and the old code is only logged. This lasts one release with no
     unexplained mismatch.
   - **Remove:** the old code and its tables are deleted.
3. **Dual acceptance for credentials.** An old credential (session JWT, `nexc_agt_` key, BYOK key
   sealed by `secret_box`) goes through three releases:
   1. The new credential works and the old one still works.
   2. The old one sits behind a flag that is off by default.
   3. The old code is deleted.
4. **Data moves by export and import, never by shared tables.**
   - The export is idempotent and resumable, and verified by row counts and content digests on
     both sides.
   - The source stays read-only for the moved tables until the flip, so a rollback is a switch,
     not a restore.
   - Tables are dropped in the remove stage, in a migration of their own.
5. **Standalone mode stays.** The product must still build, test and run with only PostgreSQL,
   Tutela in library mode, a dev OIDC issuer, and the cores' local or stub modes. CI runs that
   mode.
6. **Tests travel with the code.** A test of moved behaviour moves to the core's repository, in
   the PR that moves the behaviour. nexc-engine keeps one **contract test** per core: a stub in
   the default suite (like `ontology::stub` today), plus a live job against the real core (like
   `make test-onto-live`).
7. **No new AgentSecOps features in nexc-engine** from now on. Security fixes only (as in #20 and
   #21), until the code has moved.

---

## 3. `backend/src/`, module by module

Line counts are from `c259cb6`. Where a module mixes concerns, its files are split by
destination.

### 3.1 Top-level files and small modules

| Module | Lines | What it is | Decision |
| --- | ---: | --- | --- |
| `main.rs`, `lib.rs` | — | Entry points | **Stays.** |
| `app/` (`router`, `state`, `caches`, `workers`) | 1,352 | Router, shared state, hot-read caches, background workers | **Stays**, shrinking. `AppState` loses the clients and workers of moved modules and gains the core clients. |
| `http/` (`extract`, `problem`, `middleware/`) | — | Extractors, problem details, ETag, idempotency, rate limit | **Stays.** `extract.rs` (the `AuthUser`/`Caller` extractor) verifies Tutela tokens with `tutela-sdk` (Tutela §5 step 1). |
| `realtime/` | 854 | SSE and WebSocket fan-out through PostgreSQL `LISTEN/NOTIFY` | **Stays.** It is the product's push channel to browsers. |
| `observability/` | 798 | Metrics, latency | **Stays.** |
| `dsa/` | 1,026 | BM25, graph, LRU, priority queue, token bucket, union-find | **Stays** as product utilities. `bm25` leaves with knowledge (onto-intelli) and `priority` with the scheduler (symphony), each once its last user is gone. |
| `kernel/` (C `ffi`, `reference`) | 289 | SHA-256 and the word-hash embedding in C | **Goes with its users.** The trace hash chain goes to attache, the built-in embedding to onto-intelli. The product keeps a plain `sha2` call where it needs a digest (tickets, idempotency keys). |
| `config/` (`mod`, `vars`, `secret`, `oidc`) | 2,078 | Typed settings | **Stays.** `oidc.rs` goes to Tutela (the trust config of `tutela-sdk`). The `NEXC_LLM_*` settings follow the LLM contract (§9). |
| `cli/` (`doctor`, `init`) | 1,066 | `nexc doctor`, `nexc init`, `nexc user create`, `nexc onto migrate` | **Stays.** `user create` goes to Tutela (users are made at the issuer), and `onto migrate` is deleted once onto-intelli holds every vocabulary. |
| `orchestrator/` | 360 | Health of the runtime and of symphony | **Stays, replaced.** It becomes the health of the four cores, shown on the infrastructure page. |
| `rpc.rs` | — | Generated `nexc.runtime.v1` and `nexc.firewall.v1` services, channel pool, TLS (#21) | **Moves to attache.** Both protocols are attache's. The product keeps the channel/TLS helper only if a core it calls speaks gRPC (onto-intelli does). |
| `egress_proxy.rs` | — | `nexc egress-proxy`, the runtime's only way out | **Moves to attache** (the runtime's egress). |
| `bin/nexc_verify.rs` | 106 | Offline verifier of evidence bundles | **Moves to attache** with the ledger. Bundles exported before the move keep verifying with the published keys (§7.2). |
| `otlp/` | 425 | OTLP export of the agent trace | **Moves to attache** (it exports the ledger's spans). |
| `firewall/` | 2,380 | Agent Firewall API (gRPC), live decisions, approvals over gRPC | **Moves to attache.** |
| `gateway/` (`fetch`, `mcp`, `a2a`) | 1,902 | Tools gateway: `fetch_url`, MCP over HTTP, A2A as a tool | **Moves to attache.** |
| `a2a/` (`card`, `client`, `inbound`) | 1,542 | Agent-to-agent: the card, outbound tasks, the inbound endpoint | **Moves to attache.** |
| `alerting/` | 1,058 | Routing of security alerts to webhooks and Slack | **Moves to attache.** The product's own notifications (the inbox) are separate and stay. |
| `memory/` (`index`, `topics`, `vectors`) | 1,105 | Agent memory: extraction, index, vectors | **Moves to onto-intelli.** |
| `ontology/` | 8,234 | The product's client of onto-intelli: vocabulary, projection of issues and projects, the map, ask, facts | **Stays as the onto-intelli client**, shrinking. `migrate.rs` is deleted after the vocabulary migration. `facts.rs` (trace → facts, "ask the audit") moves to attache, which then feeds onto-intelli itself. `stub.rs` stays as the contract-test stub. |

### 3.2 `security/`

| File | Decision |
| --- | --- |
| `password.rs`, `totp.rs`, `session.rs`, `gate.rs`, `jwt.rs`, `oidc.rs` | **Tutela.** Replaced by the OIDC issuer (Keycloak in Tutela phases A–B) plus Tutela tokens. Local login stays behind a flag until the dual run passes (Tutela §5 step 7). |
| `api_token.rs` (`nexc_agt_` firewall API keys) | **Tutela**, as `tut_ak_…` agent keys. The firewall they open moves to attache. |
| `agent_token.rs` (per-run Ed25519 agent tokens, the JWKS) | **Tutela.** `tutelad` issues delegated agent tokens, requested by attache's runtime (Tutela §5 step 8). nexc stops minting tokens. |
| `secret_box.rs` (AES-256-GCM sealing under `NEXC_MASTER_KEY`) | **Tutela** (`tutela-vault`). Stored secrets are re-sealed once (§7.1), then the file is deleted. |
| `egress.rs` (PR #20: the SSRF guard that judges resolved addresses) | **Tutela** (`tutela-safenet`). Until `tutela-safenet` ships, #20's guard is the product's stand-in; it replaces `check_production_url`, the URL-text check Tutela §5 step 6 names. |
| `signing.rs` (firewall decisions, run seals, bundles) | **attache**, with the ledger. |
| `random.rs` | **Stays** (utility). |

### 3.3 `engine/`

| Files | Decision | Notes |
| --- | --- | --- |
| `issue_report.rs` | **Stays** | A finished run reports back to its issue. It becomes a finished symphony task reporting to its issue. |
| `assistant.rs`, `summary.rs` | **Stays** | The issue assistant and day summaries are product features. Their model calls go through the LLM contract (§9). |
| `editor.rs`, `deps.rs`, `analysis.rs`, `templates.rs`, `planner.rs` | **Stays** | A graph is the product's breakdown of an issue into work: editing, dependency detection, critical path, starter templates and the planner's proposal. Executing it moves (below). |
| `json_stream.rs` | **Stays** | Utility. |
| `transfer.rs` | **Stays**, shrinking | Moves a workspace's product data to the owner's database. Each moved table leaves its list (the test `every_table_is_listed_once_and_after_what_it_refers_to` keeps it honest). |
| `workspaces.rs` | **Stays**, coordinating | Erasing a workspace (its rows, its files, its evidence). After the moves it erases the product's data and asks each core to erase the org's data in it (Tutela, attache, symphony, onto-intelli). |
| `account.rs`, `sso.rs`, `two_factor.rs` | **Tutela** | Account events, SSO linking (#20 already hardened it), the second factor. |
| `credentials.rs` | **Stays, rewritten** | Resolves the LLM settings for a call: becomes the LLM contract client (§9). BYOK keys are read from `tutela-vault`. |
| `scheduler.rs` (101.8K), `lease.rs`, `commands.rs` | **texc-symphony**, once it has the prerequisites of §6 phase 4 | DAG execution, run leases and takeover (docs/HA.md), operator commands to a run. The product hands each node to symphony as a task with its dependencies (§7.3). Until symphony can hold dependencies, plan without executing and retry, these stay in the product. |
| `executor/llm.rs`, `executor/symphony.rs` | **texc-symphony** | The built-in one-call executor becomes symphony's API agent loop. The bridge becomes `symphony-tasks-client`. |
| `executor/agent.rs`, `executor/runtime_grpc.rs` | **attache** | Driving the agent runtime (NDJSON and gRPC). Symphony's tasks reach agents through attache. |
| `artifacts.rs` | **texc-symphony** | Run outputs on disk. The product keeps issue attachments (`artifact_files`), which are separate. |
| `approvals.rs`, `defend.rs`, `policy.rs`, `risk.rs`, `evidence.rs`, `trace.rs`, `recording.rs`, `guardrails.rs`, `usage.rs` | **attache** | Approvals, DLP and injection defence, policy resolution, risk and quarantine, evidence-bound outputs, the trace spine, tool-call recording, budgets, and the usage ledger (the model gateway meters calls). |
| `knowledge.rs` | **onto-intelli** | Document parsing, chunking, embedding and retrieval. |

### 3.4 `http/handlers/`

| Handlers | Decision |
| --- | --- |
| `issues`, `cycles`, `search`, `insight`, `artifact_files`, `realtime`, `health`, `latency`, `templates`, `graphs`, `nodes`, `edges`, `plans` | **Stays.** |
| `teams`, `workspaces` | **Split.** Product metadata stays: the team's key, name, workflow and issue states; the workspace's name, slug and settings, keyed by the Tutela org id. Members, invites and roles go to Tutela. |
| `settings` | **Split.** Product preferences stay. LLM settings follow the contract (§9); their keys go to `tutela-vault`. |
| `ontology` | **Stays** (it mints the onto-intelli UI session and proxies the map). |
| `transfer`, `infrastructure` | **Stays.** `infrastructure` shows the cores' health. |
| `auth`, `account`, `sso`, `agent_token_keys` | **Tutela.** |
| `platform` | **Tutela Console** for users and orgs. A small product admin page stays for product data. |
| `agents`, `agent_templates`, `policies`, `approvals`, `alerts`, `alert_destinations`, `secops`, `evidence`, `trace`, `payloads`, `exports` (SIEM pulls), `mcp_servers`, `a2a`, `a2a_counterparts` | **attache.** The console keeps the pages, reading attache's API (or embedding attache's components). |
| `runs`, `orchestrator`, `artifacts`, `sessions` (the run tree with roll-ups) | **texc-symphony** for execution and outputs; the roll-ups of agents, tokens and cost come from attache's ledger. The product keeps a read-only run view on the issue. |
| `knowledge`, `memories` | **onto-intelli.** |
| `audit` | **Split.** Membership, team and credential events go to Tutela's audit. Agent, policy and evidence events go to attache's ledger. Issue history (`issue_events`) stays. |

### 3.5 `repo/` and `domain/`

These follow their handlers and engine files. Concretely:

| Destination | `repo/` | `domain/` |
| --- | --- | --- |
| **Stays** | `issues`, `cycles`, `teams` (metadata), `artifact_files`, `insight`, `assistant`, `graphs`, `nodes`, `edges`, `plans`, `idempotency`, `outbox`, `tickets`, `onto_export`, `onto_projection`, `settings` (preferences), `workspaces` (metadata) | `issue`, `cycle`, `insight`, `assistant`, `artifact_file`, `graph`, `plan`, `template`, `context`, `prompt`, `search`, `status`, `validation`, `error`, `workspace` (metadata), `settings` |
| **Tutela** | `users`, `account_events`, `tokens` (refresh tokens), `resets`, `two_factor`, `sso`, `platform`, `agent_tokens` (API keys), the membership parts of `workspaces` and `teams` | `user`, `account`, `sso`, `platform`, `WorkspaceRole`, `TeamRole`, `WorkspaceAction`, `can()`, `TeamAccess` (`workspace.rs`) |
| **attache** | `agents`, `agent_evaluations`, `baselines`, `approvals`, `alerts`, `alert_destinations`, `alert_deliveries`, `a2a`, `mcp_servers`, `policies`, `secops`, `evidence`, `spans`, `span_payloads`, `external_runs`, `otlp_export`, `audit` (the chain), `usage` | `agent`, `agent_template`, `capabilities`, `policy`, `policy_document`, `dlp`, `injection`, `risk`, `approval`, `alert`, `alert_routing`, `evidence`, `bundle`, `seal`, `trace`, `recording`, `secops`, `session` (run tree), `mcp`, `a2a`, `guardrails`, `audit_chain`, `audit`, `usage` |
| **texc-symphony** | `runs`, `run_leases`, `run_commands`, `artifacts` | `run` |
| **onto-intelli** | `knowledge`, `knowledge_vectors`, `memories`, `onto_migrations` (deleted after the move) | `knowledge`, `memory`, `topics` |

### 3.6 `llm/`

| Files | Decision |
| --- | --- |
| `mod.rs`, `service.rs`, `retry.rs`, `sse.rs` | **Stays** as the LLM contract client (§9): one interface for the product's own calls (assistant, planner, summaries), in direct mode or gateway mode. |
| `demo.rs` | **Stays.** The offline provider for trials, tests and `make seed-demo`. It is product-local and not part of the contract. |
| `anthropic.rs`, `openai_compat.rs`, `router.rs`, `breaker.rs`, `pricing.rs`, `catalog.rs` | **attache** (the model gateway owns providers, limits, pricing and metering). In direct mode the product keeps the two adapters until every installation runs a gateway. |
| `claude_code.rs` | **texc-symphony** (its Claude Code backend). It also stops a workspace from running the operator's CLI login on the backend (an open item of #20). |
| `embeddings.rs` | **onto-intelli.** |

## 4. `agent-runtime/`

The whole runtime **moves to attache**: its agents, the runner, the per-run policy checks
(`policy.py`), approvals, injection screening, evidence, tools (`files`, `python_exec`, `docx`,
`workspace`, the gateway tool), LLM providers, and the gRPC and HTTP servers. attache PR #13
already routes every model call of attache's runtime through its model gateway.

One exception: **document parsing** (`parsing/`: PDF, DOCX, PPTX, XLSX, HTML, Markdown, tables)
moves to **onto-intelli** with knowledge, which is what ingestion needs it for. The backend's
`engine/knowledge.rs` calls the runtime's `/v1/parse` for every non-text upload, so between
phase 3 (the agent runtime leaves) and phase 5 (knowledge leaves) `parsing/` runs alone as a
parse-only service: the same image without agents, keys or egress.

Its tests (`agent-runtime/tests/`) move with it: `test_parsing.py` to onto-intelli, everything
else to attache.

## 5. The console (`frontend/`)

- **Stays:** `features/issues`, `graph`, `graphs`, `command`, `search`, `insight`, `landing`,
  `workspaces` (product parts), `settings` (product parts) and `artifacts` (issue attachments).
- **Tutela:** `features/auth` and `account`, and members and roles in `workspaces` and `settings`.
  These use `@f8fvwgzc/tutela-react` (`<Can>`, `<Field>`, `<Protect>`) with UI keys from nexc's
  `tutela.resources.toml`. `features/platform` goes to the Tutela Console.
- **attache:** `features/agents`, `secops`, `policies`, `alerting`, `a2a`, `gateway`, `audit` and
  the trace views of `runs` and `sessions`. They are embedded from attache, or kept as pages over
  attache's API until attache ships components.
- **onto-intelli:** `features/knowledge`, `memory` and `ontology`. The map is already embedded
  from onto-intelli's UI.

---

## 6. Order of the moves

Each phase starts only when the cores it needs have shipped what it uses. Each phase ends with its
remove stage, so the product never carries two copies of one concern for longer than one move.

| Phase | Move | Needs | Done when |
| --- | --- | --- | --- |
| 0 | Security fixes land: #20 and #21 (Phase 0's #19, which made `main` green, merged on 2026-10-26) | — | Merged; CI green on `main` |
| 1 | **LLM connection contract v1** (§9): the product reads `LLM_*` keys from `llm-config`/`llm-credentials` and can call attache's model gateway | attache's model gateway (attache #13) for gateway mode; nothing for direct mode | Both modes run in CI; old `NEXC_LLM_*` names are read with a deprecation warning for one release |
| 2 | **Identity to Tutela** (§7.1): token check, users, orgs, memberships and roles, vault, safenet | Tutela phase A (`tutela-sdk`, `tutela-pdp`, `tutela-vault`, `tutela-safenet`); phase B for the Console and runtime roles | 1–2 weeks of shadow role decisions with no unexplained mismatch, one release flipped, old auth deleted |
| 3 | **AgentSecOps to attache** (§7.2): agents, policies, firewall, approvals, risk, DLP, the gateways, A2A, the ledger, the runtime | Phase 2 (agent principals and delegated tokens are Tutela phase B); attache's registry, ledger import and runtime protocol; **and document parsing kept alive**: knowledge ingestion parses every non-text upload through the runtime's `/v1/parse` until phase 5 | Shadow firewall decisions match for 1–2 weeks and the ledger verifies after import. Then `agent-runtime/` is deleted **except `parsing/`**, which keeps serving `/v1/parse` as a parse-only service (no agents, no keys, no egress) until phase 5 moves it to onto-intelli |
| 4 | **Execution to texc-symphony** (§7.3): the scheduler, leases, executors, run outputs | Phase 3 (symphony tasks reach agents through attache); symphony PRs #4 (task API auth) and #5 (task API, client crate); **and three features #5 does not have, to add to symphony first:** (1) dependencies on create: a task names the tasks it waits for, and is queued only when they have succeeded; (2) a plan or dry-run endpoint that returns the order and the waiting set of a set of tasks without running them; (3) a retry policy per task (attempts, backoff, which failures are retried). Leases and takeover come with them, so a symphony replica that dies hands its tasks on. | 1–2 weeks of shadow plans (below) with no unexplained difference, then cut-over, then `scheduler.rs` deleted. Until (1)–(3) exist, phase 4 does not start and the product keeps its scheduler |
| 5 | **Knowledge and memory to onto-intelli** (§7.4) | onto-intelli's Tutela work (per-property masking, `ai_reach`, about 6–9 person-weeks by Tutela's estimate) | 1–2 weeks of shadow retrieval with agreed overlap, then cut-over, then the tables dropped |
| 6 | Clean-up: drop moved tables and settings, slim `AppState`, shrink `transfer.rs`, update `docs/` | 1–5 | The product builds with only its own modules and the core clients |

Why this order:

- **Identity first.** Every core accepts Tutela tokens, and every later move needs principals the
  cores agree on (a person, an org, an agent).
- **AgentSecOps before execution.** Symphony runs work on agents, and attache must already own the
  agents, their policies and their ledger when it does, or the trace would split in two.
- **Knowledge last.** Its prerequisite is onto-intelli's largest change (masking hidden fields
  before any prompt is built). Until then the product's knowledge base keeps its label checks.
- Tutela's own order (symphony, attache, onto-intelli, then nexc-engine) is about *adopting
  Tutela*. It fits this order: the cores adopt Tutela first, and nexc-engine's identity move
  (phase 2) comes after them.

## 7. Data migration, per move

### 7.1 Identity to Tutela

| Today | Becomes | How |
| --- | --- | --- |
| `workspaces` | A Tutela **org** | One org per workspace, whose external id is set to the workspace UUID, so every product row keeps its `workspace_id`. The product keeps `workspaces` for its own metadata (name, slug, settings), keyed by that id. |
| `WorkspaceRole` (owner, admin, member, guest) and `TeamRole` (owner, member) | **Seeded roles** in nexc's `tutela.resources.toml` | Resource types `org` and `team` (the team's parent is the org). Permissions mirror today's rules exactly: `can()`, `can_assign_role`, `can_remove_member` and `TeamAccess::{can_view, can_contribute, can_file_issues, can_join, can_manage}`. That way the shadow run measures the move, not a change of rules. |
| `workspace_members`, `team_members` | Memberships with those roles | Exported by workspace; each row is checked by count and by a digest of `(user, role)` pairs. |
| `workspace_invites` | Tutela invitations | Pending ones only; expired ones are dropped. |
| `users` | Tutela users and issuer accounts | Matched by `(iss, sub)` where an identity exists, otherwise created. Argon2id hashes go to Keycloak during the dual run if it accepts the format; otherwise users reset their passwords. The platform role (`users.role = admin`) becomes the Tutela platform role. |
| `user_identities`, `oidc_logins`, `sso_role_grants` | Federated identities and group mappings | Identities by `(iss, sub)`, never by e-mail (as #20 enforces now). The SSO group-to-role grants become Tutela group mappings. |
| `recovery_codes`, the TOTP secrets in `users` | Re-enrolment at the issuer | Not copied: a second factor is enrolled again where users sign in. |
| `refresh_tokens`, `password_resets` | Nothing | Sessions end at the flip; everyone signs in once. (`realtime_tickets` stay in the product, minted for a Tutela-verified caller.) |
| `agent_api_tokens` (`nexc_agt_`, hashed) | `tut_ak_…` keys | Hashes cannot be copied: owners mint new keys during the dual-acceptance window. |
| `account_events`, `platform_events` | Tutela audit | Exported once for the record; new events are Tutela's. |
| Secrets sealed by `secret_box` (BYOK LLM and embedding keys, MCP and A2A auth headers, alert URLs) | `tutela-vault` | A one-off re-seal job: open with the master key, seal with the vault, compare the counts, keep the old column until the remove stage. The secrets that leave with phases 3 and 5 are re-sealed by their new owner instead. |

**Role call sites.** Tutela's `INTEGRATION.md` counts about 38 decision call sites (`can()`,
`TeamAccess`, `role_of`, `find_for`). A broader search (`member_of`, `require`, `TeamAccess`,
`role_of` in `http/handlers/` and `repo/`) finds about 180 lines in 24 files, many of them
`member_of` + `require` pairs. So the shadow check goes **inside** `require()`, `TeamAccess` and
`member_of`: one place compares nexc's answer with `tutela-pdp`'s and logs a mismatch to target
`tutela.shadow`. The call sites do not each change. Lists (issues, projects, search) move to
`Pdp::list_filter` inside SQL in the flip stage, and masked fields are stripped from responses.

### 7.2 AgentSecOps to attache

| Today | How it moves |
| --- | --- |
| `agents`, `agent_evaluations`, `agent_baselines`, `agent_session_baselines`, `agent_risk_history` | attache's registry becomes the system of record. Each agent gets the principal `agent:agt_<uuidv7>` (created through `tutela.admin.v1`). Today's principal `agent://<workspace>/<name>` is kept as an alias in attache, so old traces still resolve. Baselines and risk history are copied, so risk scoring does not restart cold. |
| `policies` (with their versions) | Imported with their history. Policy version numbers are kept, because spans record the version they were decided under. |
| `approvals` | Open approvals are drained before the cut-over (decided or expired). Decided ones are imported read-only. |
| `alerts`, `alert_destinations`, `alert_deliveries` | Imported. Destination URLs are re-sealed by attache. Undelivered outbox rows are delivered by nexc before the cut-over. |
| `agent_spans`, `span_payloads`, `audit_log`, `audit_chain_heads` (the hash-chained trace and audit) | Imported **byte for byte**: hashes, seals and signer `kid` unchanged. attache publishes nexc's signing public keys as retired keys in its JWKS, so every seal and every bundle exported before the move still verifies (`nexc-verify` and attache's verifier must agree on the same bundles; that is a test). Membership events in `audit_log` go to Tutela's audit (7.1). |
| `mcp_servers`, `a2a_counterparts`, `a2a_tasks` | Imported. Auth headers are re-sealed by attache. No stdio server comes along (#20 already refuses them). |
| `workspace_guardrails`, `llm_usage` | Budgets become attache charter limits enforced by the model gateway. `llm_usage` history is imported for reporting; new usage is metered by the gateway. The product shows usage through attache's API. |
| `otlp_export_cursor` | attache's exporter starts from the imported trace's end. |

**Shadow.** For 1–2 weeks every decision nexc's firewall takes (tool, host, model, spawn, read)
is also asked of attache with the same capabilities and policy. A mismatch is logged with the
rule ids on both sides. nexc already counts `policy_mismatches` between its own runtime and its
gateway, and the same counter is reused.

**Cut-over.**
1. New runs start on attache's runtime.
2. Runs in flight finish on nexc's: the lease keeper stops claiming new runs.
3. Once none are left, nexc's runtime, gateways and firewall are switched off. The runtime's
   `/v1/parse` stays up as the parse-only service until phase 5 (§4).

### 7.3 Execution to texc-symphony

| Today | How it moves |
| --- | --- |
| `graphs`, `nodes`, `edges`, `plans` | **Stay** in the product: they are the plan of work. |
| `runs`, `node_runs`, `run_lease_events`, `run_commands` | History stays read-only in the product, or is exported to symphony's store if symphony offers an import. New runs are symphony tasks. A product table maps `(run, node)` to a symphony task id. |
| `artifacts` (run outputs on disk) | Existing outputs stay where they are, read-only. New outputs are symphony's, linked from the task. |

A node becomes a symphony task that names its upstream nodes' tasks as its dependencies
(prerequisite (1) of phase 4); symphony queues it when they have succeeded, and the product
follows each task with the SSE watch. This is not what `blocked` and `unblock` mean in symphony
#5: there, `blocked` is an agent waiting for an operator or out of budget, `unblock` re-queues a
blocked or failed task (409 otherwise), a task never retries by itself, and a create request
with unknown fields is refused. Retries, leases and takeover (docs/HA.md) become symphony's only
once prerequisites (1)–(3) are in symphony. Until then the product keeps them, and phase 4 waits.

**Shadow** (work must never run twice, so nothing executes in shadow). For 1–2 weeks, each run
also asks symphony's plan endpoint (prerequisite (2) of phase 4) to plan the same graph without
executing it. Its order and waiting set are compared with `scheduler.rs`'s waves, and any
difference is logged.

### 7.4 Knowledge and memory to onto-intelli

| Today | How it moves |
| --- | --- |
| `documents` (+ files under `<data>/documents`), `document_chunks`, `knowledge_topics`, `workspace_knowledge_settings` | Files are re-ingested by onto-intelli's ingestion (parsing moves with it). Embeddings are copied when the model matches, and re-embedded otherwise. Topics become onto-intelli collections. A document's data label (public, internal, confidential, restricted) becomes onto-intelli's sensitivity, so `ai_reach` keeps what an agent may read. |
| `memories`, `memory_topics` | Imported as agent memory, with their scope (user, graph, node) and importance. |
| `onto_vocabulary_migrations`, `onto_export_*`, `onto_projection`, `onto_retired_tenants` | Stay until the projection of issues into onto-intelli is the only link left. The vocabulary migration table is dropped once `nexc onto migrate` has nothing left to do. |

**Shadow.** For 1–2 weeks every retrieval (passages for a prompt, memories for an agent) is also
asked of onto-intelli. The product keeps using its own answer and logs the overlap of the top *k*.
The flip waits for an agreed overlap, and every missing passage is explained (a parse difference,
a label, the embedding).

## 8. Tests that come along

| Move | Tests that move to the core | What stays in nexc-engine |
| --- | --- | --- |
| Identity → Tutela | `tests/auth.rs`, `account.rs`, `oidc.rs` (+ `common/oidc.rs`, the provider stub), `platform.rs`, the membership and role parts of `workspaces.rs`, the authentication parts of `worst_case.rs`, and the unit tests of `security/` (password, TOTP, JWT, session, `secret_box`, `egress`) | A contract test of the `Caller` extractor against `tutela-sdk` in library mode; the shadow-mismatch logging test; `worst_case.rs` keeps "nobody gets in without a token" and "an outsider reads nothing of another workspace", now with Tutela tokens |
| AgentSecOps → attache | `tests/firewall.rs`, `firewall_risk.rs`, `gateway.rs`, `a2a.rs`, `alerting.rs`, `policy.rs`, `secops.rs`, `identity.rs`, `trace.rs`, `recording.rs`, `evidence.rs`, `evidence_kept.rs`, `audit.rs` ("ask the audit"), `otlp.rs`, `runtime_grpc.rs`, `grpc_tls.rs` (#21), `sessions.rs` (roll-ups), and all of `agent-runtime/tests/` except `test_parsing.py` | A contract test of the product's calls to attache (start a run, read its outcome and roll-ups) with a stub, and a live job |
| Execution → texc-symphony | `tests/engine.rs`, `ha.rs`, `artifacts.rs`, and the execution parts of `graphs.rs` | `graphs.rs` keeps graph editing, dependency detection and analysis; `issues.rs` keeps the issue ↔ graph link (`issue_report`); a contract test against `symphony-tasks-client` with a stub that covers dependency release, the plan endpoint and retries (phase 4's prerequisites) |
| Knowledge and memory → onto-intelli | `tests/knowledge.rs`, `memory_scale.rs`, `memory_vectors.rs`, and `agent-runtime/tests/test_parsing.py` | `ontology.rs` and `onto_live.rs` (the onto-intelli contract, stub and live) |
| Stays | — | `issues.rs`, `search.rs`, `insight.rs`, `realtime.rs`, `caches.rs`, `idempotency.rs`, `schema.rs`, `transfer.rs`, `api_latency.rs` (re-seeded without the moved endpoints) |

`schema.rs` (every table has a primary key, every foreign key an index) keeps running over
whatever tables are left after each phase.

## 9. LLM connection contract v1

The platform's contract: a product reads its model connection from the shared Kubernetes Secret
**`llm-credentials`** and ConfigMap **`llm-config`**, in one of two modes:

- **Direct:** `LLM_PROVIDER`, `LLM_BASE_URL`, `LLM_MODEL` (ConfigMap) and `LLM_API_KEY`, or
  `LLM_API_KEY_FILE` pointing at the Secret mounted as a file (preferred: the key stays out of the
  environment).
- **Gateway:** `LLM_GATEWAY_URL` (ConfigMap) and `LLM_GATEWAY_TOKEN_FILE` (Secret, mounted). Every
  call goes to attache's model gateway, which holds the provider key, meters and limits the call,
  and writes the record. In this mode the product is never given a provider key: `LLM_API_KEY`
  must not be mounted.

When `LLM_GATEWAY_URL` is set the product runs in gateway mode; otherwise in direct mode.

### 9.1 Today's settings onto the contract

| Today | Contract v1 | Where | Notes |
| --- | --- | --- | --- |
| `NEXC_LLM_PROVIDER` (`anthropic`, `openai_compatible`, `demo`, `claude_code`) | `LLM_PROVIDER` | `llm-config` | `anthropic` and `openai_compatible` map to the contract's provider names. `demo` stays a product-local setting for offline trials and is not part of the contract. `claude_code` leaves the product (§3.6). |
| `NEXC_LLM_MODEL` | `LLM_MODEL` | `llm-config` | |
| `NEXC_LLM_BASE_URL` | `LLM_BASE_URL` | `llm-config` | It stays an operator endpoint, reached without the SSRF guard (#20's `operator_endpoints`). |
| `ANTHROPIC_API_KEY` (Secret `nexc-secrets`, env) | `LLM_API_KEY_FILE` (preferred) or `LLM_API_KEY` | `llm-credentials` | Today it is read only for `anthropic`. Under the contract the key belongs to the configured provider, whatever it is. |
| — | `LLM_GATEWAY_URL`, `LLM_GATEWAY_TOKEN_FILE` | `llm-config`, `llm-credentials` | New: the gateway mode. |
| `NEXC_LLM_FALLBACKS` (Anthropic server-side refusal fallbacks) | — | stays product-local in direct mode | In gateway mode this is the gateway's decision. |
| `NEXC_LLM_BREAKER_FAILURES`, `NEXC_LLM_BREAKER_OPEN_SECS` | — | stay product-local in direct mode | The gateway has its own limits. |
| `NEXC_EMBED_BASE_URL`, `NEXC_EMBED_MODEL`, `NEXC_EMBED_API_KEY`, `NEXC_EMBED_DIMS` | — (not in v1) | stay until phase 5 | Embeddings move to onto-intelli with knowledge. If the contract grows an embeddings section, they map onto it. |
| `NEXC_CLAUDE_BIN`, `RUNTIME_CLAUDE_BIN` | — | leave with `claude_code` | |
| `NEXC_RUNTIME_EGRESS_ALLOW` (the runtime's egress allow-list) | derived from `LLM_BASE_URL`'s host | until phase 3 | In gateway mode the runtime (attache's) needs no provider host at all. |
| Runtime `NEXC_LLM_MODEL`, `RUNTIME_LLM_MAX_RETRIES` | `LLM_MODEL` for the default model | until phase 3 | The runtime moves to attache, which reads the contract itself. |
| Per-request `Llm{provider, model, base_url, api_key}` on the runtime stream (`runtime.proto`) | — | until phase 3 | In gateway mode no key travels to the runtime; #21's TLS protects the hop until then. |
| User and workspace BYOK (`llm_settings`, `workspace_llm_settings`: provider, model, base URL, sealed key) | Product data, outside the contract | product, keys in `tutela-vault` from phase 2 | The contract covers the **installation's** connection. A workspace that brings its own key is either passed to the gateway as a per-tenant credential (an attache API to agree) or, in direct mode, used by the product as today, behind the SSRF guard (#20). |
| Helm `config.NEXC_LLM_PROVIDER`, `NEXC_LLM_MODEL`, `NEXC_LLM_FALLBACKS`, `secrets.values.ANTHROPIC_API_KEY` | `llm-config`, `llm-credentials` | `deploy/helm`, `deploy/k8s` | The chart stops writing these into `nexc-config` and `nexc-secrets`. It mounts `llm-credentials` as files and reads `llm-config` with `envFrom`. Both objects are shared and created by the platform, not by the chart. |
| onto-intelli's `ontoIntelli.llmProvider`, `anthropicApiKeyFromSecret` (Helm) | the same contract | onto-intelli's own chart | onto-intelli reads the contract itself; nexc's chart stops passing it a key. |

### 9.2 How the product switches (phase 1)

1. The settings reader accepts the contract's names. For one release it also reads today's
   `NEXC_LLM_*`/`ANTHROPIC_API_KEY` and logs which old name is in use; when both are set, the new
   name wins.
2. `LLM_API_KEY_FILE` and `LLM_GATEWAY_TOKEN_FILE` are read once at start, like #21's PEM files,
   and kept out of `Debug`.
3. Gateway mode sends the product's own calls (assistant, planner, summaries) to the gateway, with
   the token and the binding headers the gateway asks for (attache #13: a call is bound to a run
   or refused). Product calls that are not runs need a binding of their own, to agree with attache.
4. `nexc doctor` reports the mode and whether the credential file is readable, never its content.
5. Tests: the reader (old names, new names, both, files); direct mode against a stub provider;
   gateway mode against a stub gateway that checks the token and that no provider key is sent.

## 10. Risks and open questions

- **One trace or two.** Until phase 3, nexc's ledger keeps the trace. A run that crosses the
  cut-over must finish on one side. The drain in 7.2 makes sure it does.
- **BYOK in gateway mode** needs an attache API for per-tenant provider credentials. Without it,
  a workspace's own key only works in direct mode.
- **Product calls outside runs** (assistant, summaries) must be bindable at attache's gateway, which
  today accepts only calls bound to a run (attache #13).
- **Symphony's missing features.** Phase 4 needs three features texc-symphony #5 does not have:
  dependencies on create, a plan or dry-run endpoint, and a retry policy (§6). They are symphony
  work, in symphony's repository, before phase 4 starts.
- **Run history.** Whether symphony imports old runs or the product keeps them read-only is
  symphony's call. Either works for the product.
- **Embeddings.** Re-embedding a large knowledge base costs model calls. Copy vectors when the
  model is the same.
- **Not yet fixed, independent of the migration:** local sign-up does not verify e-mail
  addresses, so with sign-up on, a pending invite can be claimed by whoever signs up with the
  address first. Tutela's issuer verifies addresses, which closes this in phase 2; before then it
  needs a fix of its own if sign-up is turned on.

## Appendix: every table and its destination

| Destination | Tables |
| --- | --- |
| **Stays** | `issues`, `issue_events`, `issue_labels`, `issue_states`, `labels`, `projects`, `cycles`, `teams` (metadata), `notifications`, `artifact_files`, `artifact_file_versions`, `artifact_folders`, `assistant_conversations`, `assistant_messages`, `day_summaries`, `graphs`, `nodes`, `edges`, `plans`, `workspaces` (metadata), `workspace_transfers`, `workspace_files`, `idempotency_keys`, `realtime_outbox`, `realtime_tickets`, `onto_export_queue`, `onto_export_cursors`, `onto_projection`, `onto_retired_tenants`, `llm_settings` and `workspace_llm_settings` (keys sealed by `tutela-vault`) |
| **Tutela** | `users`, `user_identities`, `oidc_logins`, `sso_role_grants`, `refresh_tokens`, `password_resets`, `recovery_codes`, `account_events`, `platform_events`, `workspace_members`, `workspace_invites`, `team_members`, `agent_api_tokens` |
| **attache** | `agents`, `agent_baselines`, `agent_session_baselines`, `agent_evaluations`, `agent_risk_history`, `agent_spans`, `span_payloads`, `approvals`, `alerts`, `alert_destinations`, `alert_deliveries`, `a2a_counterparts`, `a2a_tasks`, `mcp_servers`, `policies`, `audit_log`, `audit_chain_heads`, `workspace_guardrails`, `llm_usage`, `otlp_export_cursor` |
| **texc-symphony** | `runs`, `node_runs`, `run_lease_events`, `run_commands`, `artifacts` |
| **onto-intelli** | `documents`, `document_chunks`, `knowledge_topics`, `workspace_knowledge_settings`, `memories`, `memory_topics`, `onto_vocabulary_migrations` (dropped after the vocabulary migration) |
