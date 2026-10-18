# AgentSecOps on nexc-engine: assessment and roadmap

<!-- #region preface -->
_Written 2026-10-17 against commit `ac41d1b` (branch `nexc-workspaces`)._

This document answers one question: what does nexc-engine need to become a platform that
**creates, configures, traces, defends and audits enterprise AI agents** at the level SoftBank
describes for its AgentSecOps PF (four functions: agent authentication and authorization,
defend, governance, observability), and what would we build first.

It is written to be shown to that team. Section 1 is an honest inventory, section 2 answers the
design questions raised, sections 3 to 8 are the proposal, section 9 is a five-minute demo script.

<!-- #endregion preface -->
---

<!-- #region assessment -->
## 1. Where nexc-engine stands today

nexc-engine is already a **node-execution engine for agents**: a graph of tasks is planned by an
LLM, scheduled as a DAG, and each node is executed by an LLM call, by a Paperclip-style agent born
in the Python runtime, or by texc-symphony. The parts that matter for AgentSecOps:

| AgentSecOps function               | What exists today                                                                                                                                                                                                                                                                                                                                             | What is missing                                                                                                                                                                                                                                                                                                        |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Authentication / authorization** | Humans: Argon2, 15-minute JWT + rotating refresh with reuse detection, two-factor sign-in, session epochs that revoke tokens in seconds (`backend/src/security/`). Workspace roles and teams. Platform admins are isolated from workspace content. Backend to runtime: shared bearer token.                                                                   | **Agents have no identity.** An agent is a row (`name, role, model, system_prompt, reports_to, budget`) that runs under the user's credential. There is no per-agent principal, no scopes (which tools, which data, which counterpart agents, which hosts), no delegation limit except the hard-coded sub-agent depth. |
| **Defend**                         | Per-workspace guardrails: monthly token budgets, allowed providers, code execution switch, secret redaction before prompts (`backend/src/domain/guardrails.rs`). Sandboxed workspace per run, `python -I` with rlimits, untrusted context wrapped as data.                                                                                                    | No policy engine evaluated **per action**. No anomaly scoring. No quarantine of an agent. No DLP on what leaves (artifacts, external calls). No prompt-injection screen on tool results.                                                                                                                               |
| **Governance**                     | Workspace audit log of membership, teams, credentials, guardrails and knowledge changes (`backend/src/repo/audit.rs`). Platform activity log. Account security events. Issues, inbox and notifications exist.                                                                                                                                                 | Policies are one JSON blob per workspace, not versioned, not inherited (org → team → agent). No approval flow for agent actions. No data classification.                                                                                                                                                               |
| **Observability**                  | Prometheus metrics, `x-request-id`, JSON logs. Realtime events per node (`node.status`, `node.log`, `node.tokens`, `node.output`, `artifact.created`) over SSE, fanned out through PostgreSQL `LISTEN/NOTIFY`. `llm_usage` has one row per LLM call (workspace, user, graph, run, provider, model, cost). `node_runs` keeps status, attempts, tokens, output. | **No trace.** Tool calls, sub-agent spawns, data reads and policy decisions are not recorded as structured, linked events. `llm_usage` does not know the agent or node. Nothing is tamper-evident. Nothing answers "what did agent X touch this week?"                                                                 |
| **Providers**                      | Anthropic, the local Claude Code CLI, any OpenAI-compatible endpoint (Ollama, vLLM, LM Studio), offline demo. Keys encrypted with AES-256-GCM, passed per request, never stored by the runtime.                                                                                                                                                               | Model allow-list per agent (today per workspace only).                                                                                                                                                                                                                                                                 |

Two things the codebase already does right for this direction and should be kept:

- **UUIDv7 everywhere.** Every id is `Uuid::now_v7()` (69 call sites), so ids sort by time. The
  trace proposed below uses the same ids: a span id is a UUIDv7 and `run_id` is the trace id.
- **Everything is data, not code.** Node types, relation types and the ontology are per-graph
  data; guardrails are a per-workspace document. Agent capabilities and policies fit the same
  pattern.

Two things to correct in how we describe the product:

- "No authentication" is wrong. Human authentication is strong. What is missing is **agent**
  identity. That is exactly SoftBank's first function (the "employee ID card" for an agent), so it
  is the gap to lead with, not hide.
- The Python runtime (4.4k lines) is the only non-Rust service. It is where the Anthropic SDK,
  `python-docx` and the parsers live. The security design below treats it as **untrusted by
  construction**: every permission it has arrives in a signed token minted by the Rust backend,
  and every decision it makes is reported as a span the backend verifies. We do not need to
  rewrite it in Rust to get a Rust-enforced boundary.

---

## 2. Answers to the design questions

**Is nexc-engine the node-execution engine of an "enterprise execution" platform?**
Yes. The scheduler, the per-graph ontology, the agent org chart and the artifact pipeline are
the execution half. onto-intelli (`../onto-intelli`, Rust, gRPC, C ABI) is the intelligence half:
ontology, permission-filtered context, deterministic answers, audit with denials, governed action
types with risk and `confirm`. AgentSecOps is the layer that sits on both: identity, policy, trace.

**Can an "agentic wrapping" layer be added without authentication being there for agents?**
Yes, and there are exactly two seams to put it in, both already single chokepoints:

1. `backend/src/engine/executor/agent.rs` builds the runtime request. This is where the backend
   mints the agent's per-run token, filters the data the agent may see, and later records every
   span the runtime streams back.
2. `agent-runtime/src/nexc_runtime/tools/base.py`, `Tool.invoke`. Every tool call of every agent
   and sub-agent passes through this one method. This is where the scope check, the span and the
   DLP hook go. The LLM call in `agents/agent.py` is the second (and last) hook.

**Rust and C, gRPC not REST, millisecond latency: is that the right target?**
Partly. Be precise about what has to be fast:

- An LLM turn takes seconds. The security layer's budget is **less than a millisecond of overhead
  per action** so it is invisible. That means policy decisions are made **in-process at the
  enforcement point** from a signed token, never by a network round trip to a policy server.
- Trace ingestion must never block an agent: spans go into an in-memory channel, are fanned out to
  the realtime hub at once, and are batch-written to PostgreSQL. Target p99 under 1 ms from emit to
  fan-out, under 50 ms to durable and on screen.
- Audit queries ("which agents read customer documents today?") are graph queries, not LLM calls.
  onto-intelli answers deterministic questions in under 1 ms with 0 tokens; that is the right
  engine for "ask the audit".
- **gRPC** is right for machine-to-machine: backend ⇄ runtime (replace NDJSON-over-HTTP with a
  bidirectional stream carrying spans and decisions), backend ⇄ onto-intelli (it already serves
  gRPC, including client-streaming `EventService/Ingest`), and an external **Agent Firewall API**
  that other agent platforms can call (`Authorize`, `Report`, `Verify`). The browser keeps
  REST + SSE: a SPA cannot speak native gRPC, gRPC-Web adds a proxy for nothing, and the OpenAPI
  contract with generated types is a strength. "gRPC, no REST" should read "gRPC between
  services, REST only at the browser edge".
- **C** stays where it is useful: the hash chain of the trace (SHA-256 is already a C kernel in
  `backend/csrc/`) and MinHash similarity for behaviour baselines.

**Agents created by a human that act like real-world agents, configurable, no hallucination.**
Three concrete mechanisms, none of them magic:

- **Capability-typed agents.** An agent is created from a role template that carries tools, data
  scopes, model, budget and a policy. It is useful on first run because its knowledge bindings
  (knowledge topics, document labels, memory namespace, onto-intelli workspace) are part of the
  template, not something the user wires afterwards.
- **Evidence-bound answers.** Every fact an agent uses comes from a retrieval it can cite (document
  chunk id, memory id, onto-intelli entity id). A verifier rejects a final answer whose claims
  carry no citation, or marks them "unverified". This is the honest version of "no hallucination":
  grounded or refused, never silently invented. onto-intelli's deterministic path already returns
  the entity ids; the knowledge base already returns chunk ids.
- **Dynamic configuration** is the existing pattern: capabilities and policies are JSON documents
  with versions and an audit entry per change, editable live, applied on the next run.

**Which LLMs?** Anthropic API, Claude Code CLI, Ollama and any OpenAI-compatible endpoint already
work per workspace and per user. Add a per-agent model allow-list and the policy engine refuses a
run that would use anything else.

---

## 3. Target architecture

```mermaid
flowchart LR
    console["AgentSecOps console<br/>React SPA<br/>REST + SSE"]
    subgraph backend["backend (Rust, nexc)"]
        ident["agent identity<br/>principals, scopes,<br/>per-run signed tokens"]
        policy["policy engine<br/>in-process, µs<br/>allow / deny / approve / alert"]
        trace["trace spine<br/>UUIDv7 spans, hash chain,<br/>batch write + fan-out"]
        anomaly["behaviour baselines<br/>+ risk score"]
        gateway["tools gateway<br/>MCP / HTTP egress proxy<br/>DLP on the way out"]
    end
    runtime["agent runtime (Python)<br/>enforces scopes from its token,<br/>reports every action as a span"]
    onto["onto-intelli (Rust)<br/>ontology + audit facts<br/>'ask the audit', 0 tokens, <1 ms"]
    llm{{"LLM providers<br/>Anthropic / Claude CLI / Ollama / OpenAI-compatible"}}
    ext[("MCP servers, APIs,<br/>other agents (A2A, §11)")]
    pg[("PostgreSQL 17")]

    console -- "REST, SSE" --> backend
    backend -- "gRPC bidi stream<br/>(spans up, decisions down)" --> runtime
    runtime -- "tool loop" --> llm
    runtime -- "every external call" --> gateway
    gateway --> ext
    trace -- "gRPC Ingest (facts)" --> onto
    console -- "ask" --> onto
    backend --> pg
```

Five rules the design follows:

1. **Every agent action is a span.** LLM call, tool call, sub-agent spawn, data read, external
   call, policy decision. Each span has a UUIDv7 id, a parent span, the agent principal, the
   decision that allowed it, start and end time in nanoseconds, and attributes.
2. **Decisions are made where the action happens**, from a token the backend signed. No network
   hop in the hot path. The backend re-checks on receipt and flags any disagreement as an incident.
3. **The trace is tamper-evident.** Each run's spans form a hash chain; the final hash is written to
   the workspace audit log and can be anchored externally. `GET /runs/{id}/trace/verify` recomputes
   it.
4. **The runtime is untrusted.** It can only do what its token says, and the backend builds the
   agent's context (documents, memories, upstream outputs) from the agent's scopes, so data the
   agent may not see never reaches the process.
5. **Policy is data with history.** Organisation → team → agent, each level narrows, never widens;
   every change is versioned and audited; a run records which policy version governed it.

<!-- #endregion assessment -->
---

## 4. Features, by AgentSecOps function

### 4.1 Identity: the agent ID card

<!-- #region identity -->
- **Agent principal.** `agents` gains `principal_id` (a stable URN such as
  `agent://{workspace}/{name}`), `owner_user_id`, `team_id`, `purpose` (one paragraph), and
  `capabilities` (JSON): allowed tools, data scopes (knowledge topics, document labels, memory
  namespaces, onto-intelli workspaces), allowed egress hosts, allowed counterpart agents, maximum
  delegation depth and fan-out, model allow-list, budgets. `status` gains `quarantined`.
- **Per-run token.** When a node is dispatched the backend mints a short-lived signed token (reuse
  the JWT machinery in `security/jwt.rs`, separate key) carrying the agent principal, run id, node
  id, the capability snapshot and the policy version. The runtime refuses to start without it and
  checks every tool call against it. Sub-agents get a **narrowed** copy (never wider than the
  parent), which is how delegation is bounded.
- **Agent card.** The console shows what SoftBank calls the ID card: purpose, owner, team, scopes,
  model, budget left, last heartbeat, current risk score, policy version, and the agent's last
  twenty actions.
- **Done (§11):** each agent an admin exposes publishes an A2A agent card and takes A2A tasks
  through the firewall with the same principals; counterparts are external agent principals.

<!-- #endregion identity -->
### 4.2 Observability: the trace spine

<!-- #region trace -->
- **Span table** `agent_spans`: `id uuid (v7)`, `run_id`, `node_id`, `agent_id`, `principal`,
  `parent_id`, `kind` (`llm_call`, `tool_call`, `spawn`, `data_read`, `egress`, `policy`,
  `approval`), `name`, `started_at`, `ended_at`, `status`, `attributes jsonb` (tool name and
  argument digest, tokens, model, host, document ids read, bytes out, decision and rule id),
  `prev_hash`, `hash`. Indexed by `(run_id, id)` and `(agent_id, started_at desc)`.
- **Runtime emits spans** as new NDJSON lines today (`{"type":"span", ...}`, contract §8), and as
  gRPC messages once phase 4 lands. The backend's `handle_line` persists and fans them out as a
  `trace.span` SSE event, so the console shows them live.
- **Hash chain per run.** `hash = SHA256(prev_hash || canonical_json(span))` using the C kernel.
  The run's final hash is written to `audit_log` and shown on the run. A verify endpoint recomputes
  the chain and reports the first broken link.
- **Usage joins the trace.** `llm_usage` gains `agent_id`, `node_id`, `span_id`, so cost is
  attributable per agent, per tool, per policy.
- **Views.** Run trace waterfall (node → agent → turn → tool), agent activity timeline, workspace
  live feed, and "who touched what": documents, memories and hosts by agent.
- **Export.** OpenTelemetry OTLP export of spans is a thin mapping once the table exists; this
  closes the README roadmap item "OpenTelemetry tracing across backend and runtime".

<!-- #endregion trace -->
<!-- #region policy -->
### 4.3 Defend

- **Policy engine** (`backend/src/engine/policy.rs`): pure functions over (principal, action,
  context) → `Allow | Deny(reason) | RequireApproval(approvers) | Alert(severity)`. Rules are data:
  tool allow/deny, host allow/deny, data label clearance, token and cost ceilings, time windows,
  rate limits per agent (the token bucket in `dsa/` already exists), and "unusual behaviour above
  risk R". Compiled into the per-run token so the runtime evaluates the same rules locally.
- **Enforcement points:** `Tool.invoke` (tool calls), the LLM call (model allow-list, budget),
  `spawn_subagent` (depth, fan-out, counterpart allow-list), the backend context builder (data
  scopes), the tools gateway (egress hosts, DLP), artifact collection (DLP on files).
- **DLP.** Extend the existing secret scrubber with PII patterns and **data labels**: documents and
  knowledge topics carry a classification (`public`, `internal`, `confidential`, `restricted`);
  agents carry a clearance; the context builder drops what the agent may not see and the gateway
  blocks labelled content from leaving to a host not allowed for that label.
- **Prompt-injection screen.** Tool results and documents are already wrapped as data; add a cheap
  classifier pass (pattern + small model, configurable) that flags instructions inside data and
  records a `policy` span with the finding. Blocking is a policy choice, not a default.
- **Behaviour baseline and risk score.** Per agent, a rolling baseline from spans: tool calls per
  turn, tokens per turn, distinct tools, distinct hosts, spawns, denials, bytes out. Score = capped
  sum of z-scores, explained in words ("3× its usual egress, 2 new hosts"). Thresholds → `Alert`,
  `Quarantine` (status flips, running nodes are cancelled, runs refused). MinHash over the tool
  sequence of a run gives "this run looks unlike anything this agent did before".
- **Kill switches.** Pause an agent, quarantine an agent, cancel a run, revoke a token (session
  epochs already do this for humans in seconds; the same gate works for agent principals).

### 4.4 Governance

- **Policy as versioned data.** `policies` table: scope (`workspace | team | agent`), version,
  document, author, created_at, and an audit entry on every change. Each run records the
  resolved policy version. The console shows a diff between versions.
- **Inheritance that only narrows.** Resolution walks workspace → team → agent; a child may
  remove a tool, host or label, never add one.
- **Approval flow.** `RequireApproval` pauses the node (new status `waiting_approval`), creates an
  inbox item for the approvers (the notifications table and inbox page already exist), records an
  `approval` span with who decided and why, then resumes or fails the node. This also delivers the
  roadmap item "human-in-the-loop approval nodes".
- **Ownership and lifecycle.** Every agent has an owner and a team; orphaned agents (owner left)
  are paused automatically; agents report a heartbeat (already in the schema) and stale ones show
  in the console.
- **Compliance views.** "Which agents can read `restricted` data?", "which policies allow code
  execution?", "every denied action this month": all queries over the span and policy tables, and
  deterministic questions for onto-intelli.

<!-- #endregion policy -->
### 4.5 Ask the audit: onto-intelli as the central query service

<!-- #region audit -->
Spans and policy decisions are exported as ontology facts to onto-intelli over its gRPC
`EventService/Ingest` stream (or in-process through its Rust library, which removes the hop):
`agent —ran→ run`, `run —executed→ node`, `agent —used_tool→ tool`, `agent —read→ document`,
`agent —called→ host`, `agent —delegated_to→ agent`, `policy —denied→ action`. Then:

- "Which agents accessed customer documents this week?" is a deterministic graph query, 0 tokens,
  about a millisecond, permission-filtered by the asker's role.
- "Why was this run denied?" walks the cause chain the same way onto-intelli explains blockers.
- The LLM is used only to explain, with ranked facts it cites, which is the evidence-bound
  behaviour of section 2.

onto-intelli already has principals with role `agent`, restricted entities with ACLs, audit of
denials, and governed action types with `confirm` for high-risk actions. Those map directly onto
AgentSecOps governance for actions an agent takes **inside** the ontology (assign an issue, update
a record): nexc agents call them through onto-intelli's MCP server, under the same principal.

<!-- #endregion audit -->
<!-- #region templates -->
### 4.6 Creating agents that work on day one

- **Role templates** ship with capabilities, policy, knowledge bindings and an evaluation set of
  competency questions (onto-intelli's `eval` pattern). Creating an agent from a template runs the
  evaluation once and shows the score on the card.
- **Dynamic configuration** without redeploy: capabilities, policy, model and knowledge bindings
  are editable in the console; the next run picks them up; every edit is in the audit log.
- **Sub-agents inherit narrowed scopes**; the console shows the delegation tree per run.

### 4.7 Providers

No new work beyond the per-agent model allow-list. The CLI provider stays valuable for a demo
with no API key; Ollama for an air-gapped one.

<!-- #endregion templates -->
---

<!-- #region plan -->
## 5. Data model changes

| Table                                       | Change                                                                                                                                                                          |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agents`                                    | `principal_id text unique`, `owner_user_id`, `team_id`, `purpose text`, `capabilities jsonb`, `policy_version`, `risk_score real`, `quarantined_at`, status adds `quarantined`. |
| `agent_spans` (new)                         | as in 4.2, append-only, hash chained per run.                                                                                                                                   |
| `policies` (new)                            | `id`, `workspace_id`, `scope`, `scope_id`, `version`, `document jsonb`, `author_id`, `created_at`.                                                                              |
| `approvals` (new)                           | `span_id`, `run_id`, `node_id`, `requested_by principal`, `approver_ids`, `decided_by`, `decision`, `reason`, timestamps.                                                       |
| `llm_usage`                                 | `agent_id`, `node_id`, `span_id`.                                                                                                                                               |
| `node_runs`                                 | status adds `waiting_approval`; `policy_version`.                                                                                                                               |
| `documents`, `knowledge_topics`, `memories` | `label text` (classification).                                                                                                                                                  |
| `audit_log`                                 | actions add `policy_changed`, `agent_quarantined`, `agent_released`, `trace_sealed`.                                                                                            |

All ids UUIDv7. Nothing here is updated after write except `agents` and `node_runs`.

---

## 6. Protocol changes

1. **Contract §8 (now):** request gains `token` (the signed per-run agent token) and
   `capabilities`; stream gains `{"type":"span", ...}` lines. Backwards compatible.
2. **gRPC between backend and runtime (phase 4):** one bidirectional stream per run,
   `Execute(stream RuntimeEvent) returns (stream BackendCommand)`. Spans, deltas, tokens and
   artifacts go up; `cancel`, `approval_granted`, `policy_update` come down. Rust side with
   `tonic`, Python side with `grpclib`. The NDJSON path stays as a fallback for one release.
3. **gRPC Agent Firewall API (phase 4):** `AuthorizeAction`, `ReportSpan` (client-streaming),
   `VerifyTrace`, `GetAgentCard`. Any external agent platform, or a sidecar in front of one, can
   use nexc as its policy and trace service. Same principals, same policies.
4. **onto-intelli:** `EventService/Ingest` for facts; `IntelliService/Ask` for the audit box.

---

## 7. Performance targets and how we prove them

`make prove` already runs the worst-case suite; add a `prove-agentsecops` target with these
assertions, measured on the demo provider so numbers are stable:

| Measure                                                  | Target                     |
| -------------------------------------------------------- | -------------------------- |
| Policy decision, in-process (runtime and backend)        | p99 < 100 µs               |
| Span emit → realtime fan-out                             | p99 < 1 ms                 |
| Span emit → durable in PostgreSQL (batched)              | p99 < 50 ms                |
| Span emit → visible in the console over SSE              | p99 < 100 ms on a LAN      |
| Deterministic audit question via onto-intelli            | p99 < 5 ms, 0 tokens       |
| Trace verification, 10,000-span run                      | < 50 ms (C SHA-256 kernel) |
| Overhead added to a 12-turn agent run with 40 tool calls | < 0.5 % of wall time       |

Every number is printed by the test and pasted into the README's compliance table, the way the
project already documents its worst cases.

---

## 8. Phased plan

Rough effort for one engineer with Claude Code; each phase is shippable and demo-able on its own.

| Phase                      | Weeks   | Delivers                                                                                                                                        | Demo moment                                                                                                                                                     |
| -------------------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **0. Trace spine**         | 1–2     | `agent_spans`, span lines from the runtime, hash chain, `trace.span` SSE, run waterfall and agent timeline, verify endpoint, `llm_usage` joins. | Watch a run's tool calls appear live; break one row in the database; verification points at it.                                                                 |
| **1. Identity and scopes** | 2       | Principals, capabilities, per-run signed tokens, narrowed sub-agent tokens, scope-filtered context builder, agent card.                         | Give the Researcher no access to `confidential` documents; it provably never sees them, and the span says why.                                                  |
| **2. Policy and Defend**   | 2–3     | Policy engine, versioned policies with inheritance, approval flow through the inbox, DLP labels, risk score, quarantine, kill switches.         | An agent tries a tool it is not allowed; denied in 60 µs; a `restricted` document never leaves; an unusual run trips an alert; one click quarantines the agent. |
| **3. Ask the audit**       | 1–2     | Fact export to onto-intelli, the "ask" box in the console, evidence-bound answers with citations.                                               | "Which agents read customer data today?" answered in a millisecond with the entity ids.                                                                         |
| **4. gRPC and gateway**    | 2–3     | Bidirectional gRPC backend ⇄ runtime, tools gateway with MCP servers per workspace and egress policy, Agent Firewall API, OTLP export.          | An external agent asks nexc "may I call this host?" over gRPC and gets a signed decision and a span id.                                                         |
| **5. Console polish**      | ongoing | Agent registry, live feed, policy editor with diff, approvals, trace explorer, risk dashboard.                                                  | The single screen a CISO wants to see.                                                                                                                          |

Phases 0 to 2 are the core; after them nexc-engine covers all four AgentSecOps functions end to
end. Phase 3 is the differentiator (an ontology-backed audit nobody else demos). Phase 4 is what
makes it a platform other agents can sit behind, which is what SoftBank's Agent Firewall is.

<!-- #endregion plan -->
---

## 9. Five-minute demo

<!-- #region demo -->
1. **Create an agent** from the "Analyst" template: purpose, owner, scopes (knowledge topic
   `finance`, label clearance `internal`, tools `read_file write_file make_docx`, model
   allow-list), budget. The card shows its evaluation score.
2. **Run a graph** with it. The trace waterfall fills in live: turns, tool calls, a sub-agent with
   narrowed scopes, tokens and cost per span.
3. **Policy in action.** Edit the policy to require approval for `make_docx`; re-run; the node
   waits, the approval lands in the inbox, approve it, the run continues; the approval is a span.
4. **Defend.** Attach a `restricted` document to the goal; the agent's context never contains it
   and the span records the drop. Then make the agent call a disallowed host: the demo script
   starts an HTTP stub on the machine and tells the research node to read the figures from it;
   the agent's `fetch_url` goes up the gRPC stream to the backend's tools gateway, which
   performs it while the allow-list names the host (an `egress` span with the host, the HTTP
   status and the bytes; the stub sees `nexc-engine-gateway/…`, not the runtime) and refuses it
   once the host is off the list, before any connection: a `policy` span with the rule, an
   alert, the risk score rises, quarantine with one click, running node cancelled. On the
   NDJSON wire the runtime is offered no gateway tool, so the script falls back to a tool the
   policy does not allow and says so.
5. **Ask the audit.** "Which agents read restricted data this week?" "Why was run 0199… denied?"
   Both answered from onto-intelli in a millisecond with ids, then "explain" with the model, every
   sentence cited.
6. **Prove it.** `make prove-agentsecops` prints the latency table; `GET /runs/{id}/trace/verify`
   shows the chain intact, then flags the row we tampered with.

<!-- #endregion demo -->
---

<!-- #region not -->
## 10. What this proposal deliberately does not do

- **No rewrite of the runtime in Rust now.** Security does not depend on the Python process
  being honest; it depends on the token and on the backend's re-check. A Rust runtime (texc-symphony
  is the precedent) can come later for performance, not for security.
- **No gRPC to the browser.** REST + SSE stay at the edge; gRPC is service-to-service.
- **No central policy server in the hot path.** Decisions are local and signed; the central
  service is for authoring policies and querying the trace.
- **No "zero hallucination" claim.** The claim is "evidence-bound or refused", and the verifier
  that enforces it is part of the product.
- **No A2A/ACP in the first release.** The gateway and principals are designed so these protocols
  are adapters added later, as SoftBank's Agent Firewall does. (A2A came after the roadmap as
  exactly such an adapter: §11.)

<!-- #endregion not -->
---

## 11. A2A on the firewall

<!-- #region a2a -->
The Agent-to-Agent protocol is an adapter on the parts above, not a second security model.
`docs/CONTRACT.md` §10 has the wire; this is where it sits.

- **A counterpart is a principal.** A workspace registers a remote A2A agent by its card URL
  (Settings → A2A counterparts). The registration makes an external agent of the workspace,
  `agent://{workspace}/a2a:<name>`, and issues it an API token exactly as the Agent Firewall API
  issues one: that token is what the remote agent presents, so every inbound call is from a
  principal nexc knows, with a status it can quarantine and a token it can revoke. The host of
  the card is checked as the gateway checks any egress host at registration.
- **An inbound task is a run.** An admin exposes an agent (`a2a_exposed`, audited). A task sent
  to its JSON-RPC endpoint becomes a one-node run of that agent in a managed graph of its own,
  executed by the instance that took it, so everything the roadmap built applies unchanged: the
  per-run token, the context builder's data scopes, the gateway for the agent's own calls, DLP
  on its artifacts, the risk baseline, approvals, alerts. The run's chain opens with the inbound
  call as a `tool_call` span marked `inbound` (the caller's principal, the task id, a digest of
  the message) and the firewall's decision as a `policy` span: the counterpart list of the
  agent's capabilities and its delegation depth decide it, as they decide a sub-agent's birth.
- **An outbound task is an egress.** A counterpart is offered to agents as the gateway tool
  `a2a:<name>` under the policy, and the gateway performs it like `fetch_url` or an MCP tool:
  the counterpart's own allow-list of agents, the capabilities' counterpart list and depth, the
  host under the egress rules (private hosts only when named), the data labels against the
  counterpart's clearance, the DLP screen of the message; then the task, streamed when the
  remote card says so, within the A2A timeout and the gateway's size cap. The answer's files
  are artifacts of the node, screened like any other; the call is a `tool_call` and an `egress`
  span naming the remote task.
- **Depth and loops cross platforms.** A task carries `nexc.hop` and `nexc.chain` in its A2A
  metadata; every outbound call made while answering an inbound task carries one more hop and
  the chain with the agent appended. The agent's resolved `delegation.max_depth` bounds the
  hop wherever the hops happened, and a chain that already names the agent, or a callback to a
  counterpart already in it, is refused as `delegation.loop`: each refusal a `policy` span, the
  run failed before the agent starts, the task answered `failed` with the rule.
- **What the console shows.** The agent card's A2A tab (the switch, the card's addresses, the
  tasks received with who called, the hop and the refusal), Settings → A2A counterparts, and
  the trace explorer's `inbound` kind. The metric `nexc_a2a_tasks_total{direction, outcome}`
  counts tasks either way.

In SoftBank's terms: the agent card is the ID card shown outward, the counterpart's token is
the visitor's badge, the firewall decides at the door, and the trace keeps the visit on record.

<!-- #endregion a2a -->