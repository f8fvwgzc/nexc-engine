# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Operate, Single sign-on (`site/operate/sso.md`): how a sign-in goes (CONTRACT §5's single
  sign-on region), every `NEXC_OIDC_*` setting, roles from groups, the break-glass
  administrator, and configuration examples for Keycloak, Microsoft Entra ID (which sends no
  `email_verified`), Okta (the groups claim on the authorization server the issuer names) and
  Google (no groups; keep the consent screen internal), clearly
  marked as not yet tried against those providers, with what to look at when a sign-in fails.
  The Helm chart gains an `oidc` block: the settings go into the ConfigMap and the client secret
  is read from a Secret of your own (`oidc.existingSecret`, key `oidc.existingSecretKey`); the
  chart refuses to render SSO without an issuer, a client id, that Secret, or the console's
  address (`config.NEXC_PUBLIC_URL` or `oidc.redirectUrl`), and CI renders it with SSO on.
- Single sign-on tests against an OpenID Connect provider in process (`backend/tests/common/oidc.rs`:
  axum serving discovery, a key set of two RSA keys and a P-256 key made at start, a token
  endpoint that checks the client's HTTP Basic credentials, the redirect URI and the PKCE
  verifier, and userinfo), driving the whole browser trip through the real router
  (`backend/tests/oidc.rs`): an account created at the first sign-in; an existing account linked
  by a verified address, and not by an unverified one nor a second identity; a platform
  administrator, an account with two-factor sign-in and the break-glass address refused
  (`link_required`) and linked from the profile page with the password (another account's
  identity refused, the break-glass account never); an identity unlinked by an administrator, the
  account signed out and the re-created identity then linked; a bad signature,
  another audience, several audiences without `azp`, an expired, future, stale or not-yet-valid
  token, another issuer, a nonce mismatch, `alg: none` and HS256 (with the client secret too) all
  refused; a state replayed, finished in another browser, older than ten minutes, made up or
  declined at the provider; key rotation with one refetch and none for a made-up key id; ES256;
  the admin groups and the workspace mapping giving roles and taking back only what they gave,
  groups from userinfo, a sign-in without the groups claim changing no role; password sign-in off with the break-glass administrator still let in;
  `return_to` never leaving the site; a suspended account refused (its identity's last sign-in
  left as it was); `GET /auth/providers` and
  `GET /auth/identities`. `backend/tests/worst_case.rs` sends hostile values in every parameter
  and cookie of the two routes (every answer a redirect to the provider or to this site), races
  eight callbacks for one state (one session), and purges what expired sign-ins left.
  `make prove-api` drives the login (held to the write target) and the whole callback
  (informational: half of it is the provider's) against the same provider in process, each
  caller a person of their own (`docs/PERFORMANCE.md`). A vitest
  renders the login page in its three states, the break-glass form and an `sso_error`; others
  render the profile page's sign-in cards: the link form and its password, a refusal in words, and
  no password or two-factor forms for an account without a password.
- Single sign-on in the console: `GET /auth/providers` (public) says how one signs in here
  (passwords or not, registration or not, the provider and its name), and the login page shows
  "Continue with" and the provider's name (wrapping on a phone), with a key icon, above the password form when single
  sign-on is on, only the button when password sign-in is off (`/login?break_glass=1` opens the
  form for the break-glass administrator), and in words why the provider sent the browser back
  when it did (`sso_error`). "Create an account" shows only when registration is open, and the register page
  says how accounts are made when it is not. The profile page (the platform console's account
  page too) shows the linked identity, with the provider, the address it gave and the last
  sign-in, and says when the account has no password (`GET /auth/identities`), and lets an account
  with a password link the provider there (Single sign-on, *Link*, with the password; a refusal
  comes back to the page in words); such an account sees
  no password or two-factor forms and no self-delete it could never confirm, only a note that the
  provider checks both, and `POST /auth/2fa/setup` answers 409 for it.
- Single sign-on with an OpenID Connect provider: `GET /auth/oidc/login` sends the browser to the
  provider (authorization code flow, PKCE S256, a `state` and a `nonce` kept server-side for ten
  minutes, single use, bound to the browser by the `nexc_sso` cookie) and
  `GET /auth/oidc/callback` exchanges the code and verifies the ID token: an RS256 or ES256
  signature by the provider's key set (kept an hour, fetched again once for a key id it does not
  hold, at most once a minute; a failed refresh of the set or of the discovery document keeps the
  old one and asks again only after a minute, so sign-ins do not queue on a slow provider; `none`
  and HS* never pass), `iss`, `aud` (and `azp` with several
  audiences), `exp` and `iat` with a minute of leeway, and the nonce. Email, verification, name and
  groups come from the ID token, else from userinfo (an address and whether it is verified always
  from the same answer). Accounts are found by a new `(issuer,
  subject)` link table (`user_identities`): a first sign-in links an existing account only by a
  verified address and only when it has no other identity of the provider, and never a platform
  administrator's, the break-glass account or one with two-factor sign-in (`link_required`: a
  provider's word on an address alone would open those, skipping their second factor), else
  creates the account (no password; invitations accepted only for a verified address; never the
  break-glass address). Those holders link the identity from their profile page instead:
  `POST /auth/oidc/link` (their password confirms) starts the same trip, the identity that signs
  in is linked whatever address it gives, and the browser comes back to the profile page
  (`identity_taken` when it is another account's). A platform administrator lists an account's
  identities with their subjects (`GET /admin/users/{uid}/identities`) and unlinks one
  (`DELETE /admin/users/{uid}/identities/{iid}`: the account is signed out, `identity_unlinked` in
  the platform's log, `sso_unlinked` in the account's activity), from the Accounts menu's "Single
  sign-on identities"; the person's next sign-in is a first one again, which lets in someone the
  provider re-created. Refusals that turn on the account are logged at `warn` with whether an
  address was given and what `email_verified` said. The admin groups give
  the platform role and the workspace mapping gives workspace roles at every sign-in, and both take
  back what they gave once the group is gone (the role before is restored; a role changed by hand,
  a platform role given in the console and a workspace's last owner are left alone); each workspace
  change is in its audit log, by `system`, and each platform role given or taken back is in the
  platform's activity log (`GET /admin/events`, no `actor_id`, by `single sign-on (<name>)`,
  naming the group). A sign-in without the groups claim at all (an Entra ID
  overage, a mapper left out of the token) changes no role and logs a warning naming the claim;
  only a claim that is there, even empty, takes roles back. The callback starts the session exactly as a password
  sign-in does and sends the browser to a same-origin `return_to` path (another site, `//host`,
  `/\host` and encoded slashes fall back to `/app`), with no token in any address; refusals go to
  `/login?sso_error=<code>`. Suspended accounts are refused; the local second factor is not asked.
  Sign-ins, links and created accounts are in the account's activity (`sso_signed_in`,
  `sso_linked`, `sso_provisioned`), and its export lists its identities. With
  `NEXC_OIDC_ALLOW_LOCAL_LOGIN=false` password sign-in answers 403 except for the break-glass
  platform administrator, and registration 403. Expired sign-ins are purged with the other
  short-lived rows; deleting an account erases its identities (`backend/src/engine/sso.rs`,
  `backend/src/http/handlers/sso.rs`, migration `20261006000080`).
- Single sign-on settings (OpenID Connect): `NEXC_OIDC_ISSUER` turns it on (unset keeps it off
  and every other `NEXC_OIDC_*` variable is ignored), with `NEXC_OIDC_CLIENT_ID`,
  `NEXC_OIDC_CLIENT_SECRET` (redacted), `NEXC_OIDC_REDIRECT_URL` (default `NEXC_PUBLIC_URL` +
  `/api/v1/auth/oidc/callback`; any other path is refused, since the cookie that binds a sign-in to
  its browser is only sent there, and the problem of a derived one is said of `NEXC_PUBLIC_URL`),
  `NEXC_OIDC_SCOPES` (`openid email profile`; `openid` is always
  asked for), `NEXC_OIDC_DISPLAY_NAME` (`SSO`), `NEXC_OIDC_GROUPS_CLAIM` (`groups`),
  `NEXC_OIDC_ADMIN_GROUPS`, `NEXC_OIDC_WORKSPACE_ROLES` (`group=workspace:role`, the workspace
  by its id, which the platform console's workspace panel now shows; a slug is refused, since
  whoever creates a workspace of that name could take it), `NEXC_OIDC_ALLOW_LOCAL_LOGIN` (`true`), `NEXC_OIDC_BREAK_GLASS_EMAIL` and
  `NEXC_OIDC_REQUIRE_VERIFIED_EMAIL` (`true`). Every problem is reported at start-up (an
  `http://` issuer or redirect URL in production, a missing client, a scope that is not one, an
  unknown role). `nexc doctor` fetches the issuer's discovery document and its signing keys,
  checks that the document is the issuer's own, uses usable endpoints and signs with RS256 or
  ES256, names the redirect URL to register (warning when its host is not `NEXC_PUBLIC_URL`'s, or
  is this machine outside development), warns when password sign-in is off without a
  break-glass administrator and fails when that address names no account, a suspended one, one
  without a password or one that is not a platform administrator's (logged at start-up too),
  counts only the RS256/ES256 keys a sign-in would use, says why an unreachable provider could not
  be reached (each cause, the URL once), and checks that each mapped workspace exists. `nexc init` quotes
  a default with a space, which `.env` needs (`backend/src/config/oidc.rs`,
  `backend/src/security/oidc.rs`, `backend/src/cli/doctor.rs`).
- Documentation site (`site/`, VitePress): the repository's Markdown rendered with a sidebar,
  full-text search, a dark theme, mermaid diagrams and the API reference (Scalar over
  `site/public/openapi.json`, a committed export of `nexc openapi`), in six sections: Start,
  Concepts, AgentSecOps, Operate, Reference and Changelog, with a home page of three entry
  points. The prose stays in `README.md`, `docs/*.md`, `CONTRIBUTING.md`, `SECURITY.md`,
  `deploy/k8s/README.md` and the Helm `values.yaml`: pages include regions of them, marked with
  `<!-- #region -->` comments that GitHub does not show, and the links inside them are mapped to
  the page that renders the file. The build fails on a link to a page that does not exist.
  `make docs` builds and serves it; `.github/workflows/docs.yml` publishes it to GitHub Pages on
  every push to `main` and on tags (once Pages is enabled in the repository settings), and CI
  builds it on pull requests without deploying. The README carries a Docs badge, and the account
  menu of the app links to the documentation and to the server's API reference. `site/README.md`
  says how the site is assembled and carries the style guide its pages follow: sentence case,
  one idea per sentence, one sentence of purpose at the top of every page, tables for options,
  no em dashes.
- Evidence ids in every node's context (`docs/AGENTSECOPS.md` §2, evidence-bound answers;
  `docs/CONTRACT.md` §4 and §8): every piece of context a node is given carries a stable
  citation id the model is told to cite it by. A document passage is headed by
  `[doc:<chunk id>] <source>`, a memory by `[mem:<memory id>]`, an upstream result's heading
  ends with `[node:<node id>]` (the runtime renders them as the `id` attribute of its
  `<document>`, `<memory>` and `<upstream_output>` tags), and `[fact:<id>]` is reserved for
  ontology facts. Both prompt builders (the built-in executor's and the runtime's) tell the
  model to cite the id in brackets after each claim that rests on it, that several ids may
  share one bracket, and to end a statement of its own with `[assumption]`. The node's
  `data_read` span lists the ids it was told to cite (`citation_ids`), beside the chunk,
  memory and node ids it already listed. Tests: the ids are rendered in both prompts and never
  leak into the upstream titles the demo provider reads back (`backend/src/domain/prompt.rs`,
  `agent-runtime/tests/test_prompts.py`).
- Evidence-bound outputs, tests and docs: the worst-case suite checks the trace explorer's new
  `decision=evidence` filter like the four decisions before it (a bounded page across the
  20,000-issue workspace, 422 for anything that only looks like the value, nothing for
  outsiders; `backend/tests/worst_case.rs`), and `docs/AGENTSECOPS.md` §2's evidence-bound
  paragraph now says the promise holds for every agent, and how.
- The demo providers cite what they were given, so the feature shows without an API key:
  the built-in demo provider reads the passages, memories and upstream results back out of
  the prompt by their ids and answers with an **Evidence** section quoting each with its id,
  marks the rest of what it says as assumptions and puts its label and summary in quotes
  rather than claims; the runtime's demo provider does the same in its final answer, its
  research notes and its documents (an Evidence section in the `.docx`, each section of the
  compiled final report citing the upstream node it rests on), and the Sources section
  follows. A node given something is `verified` with full coverage; one given nothing is
  `unverified` with nothing forged. `make demo-secops` gains step 3, **Evidence**: the
  board-compensation document is attached as `internal`, the research node cites its passages
  and is verified (the run page shows the chips, the node's `evidence` span carries the
  verdict); relabelled `restricted`, the same node runs without it, the coverage drops to 0 %
  and the Analyst template's policy marks the output (the `evidence` span's decision is `alert`);
  the later steps moved down one. Tests: the built-in provider's output verifies against what
  the prompt gave it and declares everything when given nothing (`backend/src/llm/demo.rs`); a
  demo run's downstream node is verified and its first node unverified, with the verdicts on
  the spans and the policy's `allow` on the record (`backend/tests/evidence.rs`); the
  runtime's demo cites the ids, keeps the Sources section and cites the upstream node in the
  compiled report (`agent-runtime/tests/test_demo.py`).
- Evidence on every surface: the run page's node table shows each output's verdict as a badge
  with its coverage (hover for the counts), and an **Evidence** section lists per node what the
  output cites as chips, each opening what it points at (a document passage in a dialog on the
  Knowledge page, `?passage=<chunk id>`, read from the new `GET
  /workspaces/{wid}/knowledge/passages/{cid}`; a memory on the Memory page, `?memory=<id>`;
  an upstream node's panel on the canvas, `?node=<id>`; the audit page for a fact), with the
  ids the node was never given crossed out and named. The node output view on the canvas
  shows the verdict and the chips read off the output itself, live. The agent card's Overview
  gets an **Evidence** section (`AgentCard.evidence`: how many of its last twenty outputs were
  verified, partial or unverified, and the list, each opening its run). The AgentSecOps
  overview counts the day's outputs by verdict (`SecopsOverview.evidence_today`; the stat row
  shows **Unverified today**), the trace explorer's Decision filter gains **evidence verdict**
  (`decision=evidence`: the `evidence` policy spans, one per output, whatever was decided on
  it), its rows and the waterfall say the verdict, and the resolved
  policy view lists the three evidence rules. Deliverables keep their citations readable:
  the runtime's `finish` appends a `## Sources` section naming every cited id with its title
  (the passage's source, the memory's first words, the upstream node's title), `write_file`
  does the same for a `.md` file and `make_docx` adds a `Sources` section to the document
  (`docs/CONTRACT.md` §8); the verifier does not count those lines as claims. Tests: the chips
  parse ids as the backend does and link where they should, the badge shows the verdict and
  the coverage, the unknown ids are listed (`frontend/src/features/runs/components/evidence.test.tsx`);
  the runtime's Sources sections on the answer, a Markdown file and a `.docx`
  (`agent-runtime/tests/test_evidence.py`); the overview counts, the explorer filter, the card
  and the passage endpoint (`backend/tests/evidence.rs`).
- The evidence verifier (`docs/AGENTSECOPS.md` §2; `docs/CONTRACT.md` §4): once a node
  answered, the backend finds the citations in its output and its Markdown artifacts (an id
  inside a code block or a code span is not a citation; a bracket holding anything but ids,
  such as a source name or a link, is not one either), checks each against what the node was
  given (an id it was neither given nor saw written inside that text, such as an upstream
  output citing its own sources, is forged; a memory is stored without the citation ids and
  markers of the output it was read from), approximates the claims as the sentences,
  bullets and table rows of the prose (headings, block quotes, code and any section whose
  heading starts with Sources, References, Citations or Bibliography aside; a claim ending
  with `[assumption]` is the model's own and counted apart),
  and reads off the coverage (claims citing what was given over the claims not declared the
  model's own) and a verdict: `verified` when the coverage reaches the policy's
  `min_coverage` (0.8 unless set) and nothing was forged, `unverified` when no claim rests on
  what was given, `partial` otherwise. The result is stored on the node run
  (`NodeRun.evidence`: the verdict, the coverage, the counts, the ids cited and the ids never
  given) and recorded as the node's own `policy` span named `evidence`, one per attempt,
  emitted once the verifier ran: the same `evidence_*` attributes (the verdict, the coverage
  as a whole percent, the counts, the ids), decision `allow` when verified and `alert`
  otherwise, with `evidence_required` and `evidence_action` saying what the policy in force
  asked. The built-in executor's one `llm_call` carries the attributes beside it; the agent
  runtime's spans are written the moment they arrive, in order, so the sessions tree and the
  waterfall stay live while a node runs (an approval waited for, a sub-agent at work). A
  cached result keeps the verdict it got. The policy document gains
  `evidence: {required, min_coverage, action: mark | reject}`,
  resolved like the other rules (required once any level requires it, the highest coverage,
  reject once any level says so; a version that would weaken the level above is refused):
  with `required` on, the `evidence` span is the decision (`mark` leaves an unverified or
  partial output at decision `alert` and the node succeeds;
  `reject` denies it and fails the node with `policy: evidence <verdict>: <why>` as the reason
  the run page shows, its output kept off the node and nothing built on it downstream). The
  default is off, so nothing changes for existing workspaces beyond the verdict being
  recorded; the Analyst, Researcher and Reviewer templates require evidence and mark what
  falls short.
  Tests: the parser and the coverage heuristic (`backend/src/domain/evidence.rs`: ids inside
  code are ignored, duplicates count once, forged ids are violations that keep an output from
  `verified`, declared claims are counted apart, the sources section is not a claim); the
  rule resolves and narrows like the others; a scheduler run under `reject` fails the node
  with the reason, skips what depended on it and leaves the verdict on the record, under
  `mark` succeeds with the verdict as an alert span, and under the default only records it
  (`backend/tests/evidence.rs`).
- The Sessions page (`/app/sessions`, in the sidebar under Build): the workspace's runs as a
  table with status, the agents that worked, nodes, model turns and tool calls, tokens, cost,
  duration and start, narrowed by agent, status and time, paged. Opening one shows the run as an
  outline that follows its graph's `trace.span`, `node.status` and `run.finished` events: the
  run, its nodes, the agent each ran as and the sub-agents it spawned, each row with its kind,
  status, duration, tokens in, out and cached, cost, turns, calls and denials (the rollup of its
  subtree; an agent's own share shown apart), a Stop behind a confirmation on every sub-agent
  still running (the run goes on) and on the run, and filters by agent, span kind and status that
  keep the ancestors so the outline stays a tree. Above it, the run's totals and a breakdown by
  model. Selecting a row shows its spans on the right as a timeline; a span opens on its digests
  and, when the run was recorded, on the prompt, completion, tool input and output as they were
  kept (monospace, folded past 4 KiB, copied with a click, every redaction and cut marked), else
  a hint to enable recording in the guardrails. `?span=` opens the outline at the row holding
  that span. Arrow keys travel the rows, Right and Left open and fold, Enter selects, Home and
  End jump; at phone width a row opens its spans in place of the outline, with a way back. Tests
  (`frontend/src/features/sessions`): the outline flattens and filters as the tree says, the tree
  shows every row with its figures, stops behind a confirmation and only for those who may, and
  travels with the keyboard; the viewer marks the redactions, copies, folds and opens long
  payloads folded.
- Opt-in recording of prompts, completions and tool calls (`docs/CONTRACT.md` §4, §5, §8). The
  guardrails gain `recording` `{prompts, completions, tool_io, retention_days, redact_labels}`,
  off by default (Settings → Guardrails has the switches, the retention and the labels; every
  change is audited as `recording_changed` with the setting in words). When a kind is on, the
  per-run token tells the runtime (`recording` claim) and the runtime sends what each span said
  up the stream as `payload` events: every turn's prompt (the system prompt and the task first,
  then what was added since the last turn) and completion (the text and the tool calls with
  their arguments), every tool call's input and output; the built-in executor records its one
  call, and the gateway records what its calls got and gave. The backend keeps only the kinds
  the workspace asked for, whatever a runtime sends, after the guardrails' secret scrubber and
  the label redaction (passages and memories of a label in `redact_labels` are replaced by their
  citation id: `[redacted restricted passage <chunk>]`), AES-256-GCM under the master key bound
  to the span id, in the new `span_payloads` table; spans themselves still carry digests only.
  The `data_read` span of every node notes `recording`, `recording_kinds` and
  `recording_redact_labels`; `Session.recording` says whether a run was recorded. A worker
  deletes payloads past the workspace's `retention_days` (1–3650, default 30) every minute;
  a transfer leaves payloads behind (documented and exempted in the coverage test); the account
  export is unchanged. `GET /spans/{id}/payload` answers the decrypted payloads of a span with the
  classifications of the data its node was given for the viewer's banner (the run's owner and
  the workspace's admins; audited as `payload_viewed`). `make proto-runtime` and `make openapi`
  regenerated the stubs and the frontend types. Tests (`backend/tests/recording.rs`,
  `agent-runtime/tests/test_trace.py`, `test_policy.py`): nothing travels or is kept unless the
  token asks, payloads a leaky runtime sends anyway are dropped, the kinds asked for and only
  those are kept and read back (sealed on disk), a restricted passage is replaced by its citation
  and a credential scrubbed, the gateway's calls are recorded, a bad retention is refused, the
  change is audited, reading is audited, members are refused, retention deletes old rows under
  the workspace's and the default retention, and the runtime's turns, tool calls and sub-agent
  tokens carry the claim.
- Stop one subtree: `POST /runs/{id}/agents/{span_id}/cancel` `{reason?}` stops the sub-agent
  born in that `spawn` span while the run goes on (whoever may cancel the run; 202 with
  `AgentCancel`; 404 for a span that is no spawn, 409 for a run that is not running). The
  command is recorded (`run_commands`, copied by a transfer) and goes down the node attempt's
  gRPC stream as the new `cancel_agent` command at once when the run executes on this instance,
  else by the instance executing it within a second (the run's cancel watcher takes what was
  recorded elsewhere, `SKIP LOCKED`, so no command is delivered twice). The runtime cancels the
  sub-agent's task (its own sub-agents with it): the parent's `spawn_subagent` call ends as the
  tool error `sub-agent … was cancelled by an operator[: reason]`, its `tool_call` span in error,
  and the parent continues; a spawn naming no live sub-agent changes nothing. On the trace the
  stop is a `policy` span of the sub-agent's principal under its spawn (`decision: cancelled`,
  `rule: operator`, `action: cancel_agent`, `spawn_span_id`, `requested_by`, `delivered`), the
  session tree shows the subtree `cancelled`, and the audit log records `agent_cancelled`
  (subject the sub-agent's principal; detail the run, the node, the spawn and the reason). The
  whole run's cancel is unchanged. `docs/CONTRACT.md` §5 and §8; `make proto-runtime`
  regenerated the runtime's stubs. Tests: the runtime stops one sub-agent on the command and the
  parent finishes with the tool error, a cancel naming nobody changes nothing
  (`agent-runtime/tests/test_grpc.py`); over the stream the stop is accepted, delivered, on the
  trace, in the audit log and on the tree, refused for a span that is no spawn, an outsider, an
  oversized reason and a finished run, and the chain still verifies (`backend/tests/sessions.rs`).
- Sessions (`docs/CONTRACT.md` §4, §5): a run seen as a tree. `GET /runs/{id}/sessions` answers
  `Session`: run → nodes → the agent each node ran as → the sub-agents it spawned (one per `spawn`
  span) → their turns and tool calls, with status (`queued`, `running`, `waiting_approval`,
  `succeeded`, `failed`, `cancelled`, `skipped`), started and ended times, duration, tokens in, out
  and cached, cost, model turns, tool calls, denials and the ids of its spans at every level, plus
  the run's totals and a breakdown by model. A sub-agent's status follows its parent's
  `spawn_subagent` call, an operator's cancel or a pending approval of its own; a subtree's cost
  is estimated from each turn's model and tokens and scaled so the subtrees of a finished node sum
  to the cost its result recorded. `GET /workspaces/{wid}/sessions?agent=&status=&from=&to=&limit=&before=`
  lists the workspace's runs with the same rollups and the agents that worked in them, newest
  first, narrowed by agent, status and time, paged (`node_runs_agent_idx` serves the agent
  filter). Computed from `agent_spans`, `node_runs` and `llm_usage`; nothing is stored twice.
  Tests (`backend/tests/sessions.rs`, `domain::session`): nested spawns roll up to their parents
  with the costs adding up, a cancelled subtree and a waiting approval are told apart, the list
  narrows by every filter, pages without repeats, clamps its limit, refuses a window the wrong way
  round, guests and outsiders; the worst-case suite reads the new routes.
- `make prove-api`: the API's latency per endpoint under load, measured and gated. A Rust
  measurement test (`backend/tests/api_latency.rs`, `--ignored`, run in release like the other
  prove targets) seeds a workspace with one of everything on the demo provider, then drives every
  documented `GET` and the main writes (an issue filed, a node added, a run started) with 64
  concurrent callers for a fixed number of requests each through the real router in process
  (no socket between the caller and axum, so the figures are the server's own, PostgreSQL
  included), and measures the three gRPC services over loopback: the runtime's `Execute` stream
  against the stand-in runtime, onto-intelli's `Ask` against the stub server, and the Agent
  Firewall's `AuthorizeAction` in process and over the wire, with `grpc.health.v1/Check` on the
  same server as the transport alone. Every row is the best of two rounds and shows p50, p99 and
  max; the targets at p99 are 20 ms for reads (200 ms for the whole-workspace aggregates, the
  SecOps overview, search, the map, usage, the day summary, transfers, knowledge search and
  memory topics, and for the server's status page, which probes what it runs on), 50 ms for
  writes and 1 ms for the gRPC calls with the work itself excluded, and a
  missed target fails the test, so the table gates CI. The first rows show the floor under every
  read, PostgreSQL's own round trip at one caller and at 64 with the harness's pool and with the
  server's default pool of 20. `NEXC_PROVE_API_CALLERS`, `NEXC_PROVE_API_REQUESTS`,
  `NEXC_PROVE_API_ROUNDS`, `NEXC_PROVE_API_DB_CONNECTIONS` and `NEXC_PROVE_API_TARGET_SCALE`
  size it for a smaller machine. The table, the machine and the build it was taken on are in
  `docs/PERFORMANCE.md`; the API's per-account rate limit became a setting on the way
  (`NEXC_API_CALLS_PER_MINUTE`, 600 as before) so a load test can lift it on its own server.
  Recorded at the end of the roadmap, the whole table in `docs/PERFORMANCE.md`: Apple M2 Pro,
  release build, PostgreSQL 18.3 native on the same machine (in Docker Desktop its round trip is
  0.65 ms instead of 0.05 ms), 64 callers × 8 requests, the best of two rounds; every gated row
  met its target.

  | endpoint | calls | status | p50 ms | p99 ms | max ms | target p99 | result |
  | --- | ---: | --- | ---: | ---: | ---: | ---: | --- |
  | PostgreSQL round trip, SELECT 1 through the pool (1 caller) | 512 | OK | 0.05 | 0.10 | 0.17 | — | info |
  | PostgreSQL round trip, SELECT 1 through the pool (64 callers, 64 connections) | 512 | OK | 1.66 | 2.11 | 2.20 | — | info |
  | PostgreSQL round trip, SELECT 1 through the pool (64 callers, 20 connections, the server's default pool) | 512 | OK | 1.53 | 3.67 | 3.71 | — | info |
  | gRPC AgentRuntime/Execute round trip, stand-in runtime (1 caller) | 512 | OK | 0.07 | 0.08 | 0.09 | 1 ms | pass |
  | gRPC IntelliService/Ask, stub onto-intelli (1 caller) | 512 | OK | 0.05 | 0.06 | 0.13 | 1 ms | pass |
  | gRPC AgentFirewall/AuthorizeAction in process: the work (token lookup, decision, span; 1 caller) | 512 | OK | 0.15 | 0.32 | 0.34 | — | info |
  | gRPC AgentFirewall/AuthorizeAction over the wire, whole call (1 caller) | 512 | OK | 0.21 | 0.38 | 0.39 | — | info |
  | gRPC grpc.health.v1/Check on the firewall server: the transport, no work (1 caller) | 512 | OK | 0.03 | 0.05 | 0.06 | 1 ms | pass |
  | GET /.well-known/agent.json | 512 | 200 | 6.19 | 7.06 | 7.23 | 20 ms | pass |
  | GET /api/v1/admin/infrastructure | 512 | 200 | 33.39 | 62.26 | 71.77 | 200 ms | pass |
  | GET /api/v1/agents/{id} | 512 | 200 | 5.67 | 6.52 | 6.69 | 20 ms | pass |
  | GET /api/v1/auth/me | 512 | 200 | 3.24 | 3.79 | 3.84 | 20 ms | pass |
  | GET /api/v1/graphs/{gid} | 512 | 200 | 10.03 | 11.40 | 11.85 | 20 ms | pass |
  | GET /api/v1/issues/{iid} | 512 | 200 | 3.67 | 4.23 | 4.67 | 20 ms | pass |
  | GET /api/v1/runs/{rid} | 512 | 200 | 8.41 | 9.33 | 9.56 | 20 ms | pass |
  | GET /api/v1/runs/{rid}/trace | 512 | 200 | 7.17 | 8.15 | 8.70 | 20 ms | pass |
  | GET /api/v1/workspaces/{wid} | 512 | 200 | 3.19 | 3.93 | 4.08 | 20 ms | pass |
  | GET /api/v1/workspaces/{wid}/guardrails | 512 | 200 | 3.38 | 3.94 | 4.00 | 20 ms | pass |
  | GET /api/v1/workspaces/{wid}/issues | 512 | 200 | 6.02 | 7.57 | 7.91 | 20 ms | pass |
  | GET /api/v1/workspaces/{wid}/llm | 512 | 200 | 3.10 | 3.52 | 3.54 | 20 ms | pass |
  | GET /api/v1/workspaces/{wid}/map | 512 | 200 | 6.39 | 7.60 | 8.26 | 200 ms | pass |
  | GET /api/v1/workspaces/{wid}/policies/resolve | 512 | 200 | 4.09 | 4.77 | 4.99 | 20 ms | pass |
  | GET /api/v1/workspaces/{wid}/search | 512 | 200 | 14.99 | 16.18 | 16.72 | 200 ms | pass |
  | GET /api/v1/workspaces/{wid}/secops/overview | 512 | 200 | 42.09 | 45.41 | 48.04 | 200 ms | pass |
  | GET /.well-known/agent.json (hot-read cache off) | 512 | 200 | 8.59 | 9.57 | 9.77 | — | info |
  | GET /api/v1/agents/{id} (hot-read cache off) | 512 | 200 | 14.28 | 15.54 | 15.83 | — | info |
  | GET /api/v1/workspaces/{wid}/guardrails (hot-read cache off) | 512 | 200 | 5.31 | 6.23 | 6.32 | — | info |
  | GET /api/v1/workspaces/{wid}/llm (hot-read cache off) | 512 | 200 | 5.12 | 5.74 | 6.09 | — | info |
  | GET /api/v1/workspaces/{wid}/policies/resolve (hot-read cache off) | 512 | 200 | 12.53 | 13.63 | 13.83 | — | info |
  | POST /api/v1/workspaces/{wid}/teams/{tid}/issues (a team per caller) | 512 | 201 | 15.49 | 16.71 | 17.17 | 50 ms | pass |
  | POST /api/v1/workspaces/{wid}/teams/{tid}/issues (64 callers on one team: the numbering lock) | 512 | 201 | 17.80 | 98.18 | 142.54 | — | info |
  | POST /api/v1/graphs/{gid}/nodes | 512 | 201 | 13.53 | 15.06 | 16.29 | 50 ms | pass |
  | POST /api/v1/graphs/{gid}/runs | 512 | 202 | 35.36 | 37.74 | 39.55 | 50 ms | pass |
- Conditional reads (`docs/CONTRACT.md` §3): every `GET` that answers JSON carries a strong
  `ETag` (the first 128 bits of the body's SHA-256; `GET /graphs/{gid}` keeps its own tag over
  the graph and its version) and `Cache-Control: private, no-cache`; a `GET` with `If-None-Match`
  naming the current tag is answered `304 Not Modified` with no body; a changed resource gets a
  new tag; streams, downloads, the probes and `/metrics` carry none and keep `no-store`. One
  middleware does it inside the metrics (a 304 is counted as one) and inside compression (the tag
  is the body's, whatever encoding it travels in). The console's fetch layer keeps the tag and
  the body of every GET by URL for the page, sends `If-None-Match` on the next request and reuses
  the kept body on a 304; the lists query-persist keeps between visits are kept with their tags
  too, per account and build, so the first refetch of a visit is a 304 as well; nothing else
  reaches storage (`frontend/src/lib/api/etag-cache.ts`). The worst-case suite sweeps every
  documented read: tagged, answered 304 with no body and the same tag, a changed issue and its
  list retagged, the SSE stream and `/metrics` untagged.
- Hot-read caches in process (`backend/src/app/caches.rs`): agent cards (the agent, its owner,
  team and evaluation; the recent spans stay live), resolved policies per agent and policy
  version, the latest policies of a workspace, workspace guardrails and the workspace's LLM
  credential are kept per instance and dropped on write: the write drops the entry locally and
  sends the key over the realtime fan-out (PostgreSQL `NOTIFY`, the channel the events travel
  on), so every replica drops it at once; `NEXC_CACHE_TTL_SECS` (60) only bounds a missed
  notification and `0` turns the caches off. The Agent Firewall decides from the shared cache
  (its own five-second policy cache is gone: a policy edited elsewhere applies the moment the
  notification lands, and the token and the agent's status are still looked up on every call);
  the scheduler, the risk scorer, the A2A card and inbound tasks read the latest policies from
  it. `nexc_cache_lookups_total{cache, result}` counts hits and misses. Tests: the entries, the
  drop by key and the wire form; two servers on one database, a write on one dropping the entry
  on the other within the second and the new policy applying there; the caches off. The gain in
  the table: `GET /workspaces/{wid}/policies/resolve` p50 4.2 ms with the cache, 12.6 ms without;
  guardrails 3.3 ms against 5.4 ms; the workspace credential 5.2 ms against 5.5 ms; an agent card
  13 ms against 14 ms (two queries left: the membership and the recent spans).
- Service to service. Every gRPC channel the backend opens (the runtime's stream, onto-intelli,
  the OTLP collector) is built by `rpc::channel`: `NEXC_GRPC_POOL_SIZE` (2) connections per
  channel with HTTP/2 keepalive (`NEXC_GRPC_KEEPALIVE_SECS` 30, dropped after
  `NEXC_GRPC_KEEPALIVE_TIMEOUT_SECS` 10 unanswered), requests and streams balanced over them,
  kept for the life of the process (the OTLP export used to open a channel per pass); the Agent
  Firewall server pings its clients the same way; `docker-compose.yml` and the Helm chart
  (`backend.grpc`) carry the settings with the reason to shorten them behind a NAT or a mesh.
  `nexc_grpc_client_latency_seconds{service, method}` times every call this backend makes (the
  runtime stream's opening, each onto-intelli call, each OTLP export). Calls to a network LLM
  provider go through a circuit breaker per endpoint (`llm::breaker`, keyed `provider@base_url`):
  after `NEXC_LLM_BREAKER_FAILURES` (5) consecutive provider failures (429, 5xx, no connection)
  the circuit opens and every call fails at once with the retryable `LlmError::CircuitOpen` for
  `NEXC_LLM_BREAKER_OPEN_SECS` (30), so a dead provider fails fast instead of each call waiting on
  its own retries and timeouts; then one probe goes out and its outcome closes the circuit or
  opens it again; `nexc_llm_breaker_total{provider, event}` counts the events and `0` failures
  turns it off. Tests: the breaker's states, one probe at a time, a dead endpoint opening its
  circuit through the router while another endpoint's stays closed.
- The latency panel: `GET /admin/latency` (platform administrators) answers p50, p99 and max per
  endpoint over the last five minutes from this instance's in-process histograms (every HTTP
  route by its template, every gRPC call the backend made; a log-spaced histogram per route and
  minute, percentiles read to within a 20 % bucket), slowest p99 first; the Infrastructure page
  shows it as a small table under the server's status, refreshed every 30 seconds.
  `deploy/grafana/nexc-engine.json` is a Grafana dashboard over the existing Prometheus metrics
  (HTTP rate, status and latency, the share of reads answered 304, the gRPC client latencies,
  runs, nodes, spans and the fan-out, firewall decisions, gateway calls, LLM tokens, the circuit
  breaker, the cache hit ratios, alert deliveries, A2A tasks), with a README on importing it; CI
  checks that it is valid JSON.
- CI runs `make prove-api` as a smoke on every push (8 callers × 4 requests, one round, the
  targets scaled ten times for a shared runner) and writes the table to the job summary.
- A2A adapter, the console: the agent card gets an **A2A** tab with the switch that publishes
  the agent (admins; disabled for an external agent, with the reason), the addresses of its card,
  its well-known card and its JSON-RPC endpoint under the console's origin with a copy each, and
  the tasks it received (the title, who called, the hop with the chain on hover, the state or the
  rule that refused it, each opening the run that answered); **Settings → A2A counterparts**
  mirrors the MCP servers page: the list with the remote's name from its card, whether it
  streams, the clearance, which agents may call it and the principal its calls act as, a Check
  that fetches the card again and a Remove behind a confirmation, and a Register dialog (name,
  card URL, header, the agents ticked, the clearance) that shows the counterpart's token once
  with a copy; the trace explorer's Kind filter gains **Inbound A2A task** (`kind=inbound`,
  tool calls marked inbound; `SecopsSpan.inbound` marks the rows). `make openapi` regenerated
  the frontend types; the drift check covers `A2aTask` and `A2aCounterpart`. Tests
  (`frontend/src/features/agents/components/agent-a2a.test.tsx`): the switch publishes, the
  addresses carry the principal percent-encoded, the task list shows the caller, the hop and
  the refusal, members and external agents get the switch disabled with the reason.
- A2A adapter, delegation bounds across hops (`docs/CONTRACT.md` §10): a task carries in its
  A2A `metadata` the number of nexc-mediated hops before it (`nexc.hop`, 0 for a counterpart's
  own call) and the principals it passed through (`nexc.chain`); every outbound call made while
  a run answers an inbound task carries `hop + 1` and the chain with the agent appended, so the
  agent's resolved `delegation.max_depth` bounds the chain as it bounds a sub-agent's depth,
  wherever the hops happened: an inbound task past the depth is refused as
  `delegation.max_depth` before the agent starts, an outbound call that would be the hop too
  many is refused by the gateway the same way, and a task whose chain already names the agent
  (or a callback to a counterpart already in the chain) is refused as `delegation.loop`; each
  refusal is a `policy` span of action `counterpart` with the hop and the chain, and the
  console's task list shows the rule. Test: the stub's task makes the Analyst call the stub
  back at hop 1 with the chain on the inbound run's trace; the task coming back with the
  Analyst in the chain is refused as a loop; two hops in is too deep for an agent allowed one,
  one hop in is admitted but its callback refused; with two allowed the callback goes out at
  hop 2 with the whole chain.
- A2A adapter, counterparts and the outbound tool (`docs/CONTRACT.md` §10): a workspace
  registers remote A2A agents (Settings → A2A counterparts; `GET/POST
  /workspaces/{wid}/a2a-counterparts`, `GET/DELETE …/{cid}`, `POST …/{cid}/check`): a name, the
  card URL (its host checked as the gateway checks egress: http(s) without credentials, no
  private or loopback host unless `NEXC_A2A_ALLOW_PRIVATE_HOSTS=true`, not denied by the
  workspace policy's egress rules), an `Authorization` header kept encrypted, the local agents
  that may call it and a data clearance. Registering fetches and validates the card (a name,
  an http(s) `url`, `skills`) and keeps it, and makes the counterpart an external agent
  principal of the workspace (`agent://{workspace}/a2a:<name>`) with its first API token in the
  answer once: that is what the remote agent presents when it sends the workspace's exposed
  agents tasks. Removal takes the principal with it. Audited as `a2a_counterpart_added` and
  `a2a_counterpart_removed`. Agents call a counterpart as the built-in gateway tool
  `a2a:<name>` (`{message, session_id?}`), offered when the capabilities name it and the
  counterpart takes tasks from the agent, and performed by the gateway under the policy: the
  counterpart's allow-list of agents, the capabilities' `counterparts` list and the delegation
  depth at the next hop, the host under the egress rules, the labels against the counterpart's
  clearance, the DLP screen of the message, then `tasks/sendSubscribe` read as server-sent
  events when the card says it streams (else `tasks/send` and `tasks/get` until the task ended)
  within `NEXC_A2A_TIMEOUT_SECS` and the gateway's size cap. The remote's text comes back to
  the model, its files are kept as artifacts of the node at `a2a/<name>/<file>`, and the call
  is a `tool_call` span (`a2a`, `direction: outbound`, the counterpart, the remote task id and
  state, the hop and the chain) with an `egress` span of transport `a2a` under it; every
  refusal is a `policy` span naming the rule. Tests: registration fetches the card, makes the
  principal, lists to members, refuses members, duplicates, bad input, private hosts without
  the setting and hosts the policy denies, and removal ends the token; an Analyst calls the
  stub through the gateway, streamed and polled, with the hops in the metadata, the file kept
  and the spans on the trace; a counterpart off the list, a host off the list and a message
  after restricted data are refused as policy spans without a byte reaching the remote.
- A2A adapter, inbound (`docs/CONTRACT.md` §10): an admin publishes a workspace agent over the
  Agent-to-Agent protocol (`PATCH /agents/{id}` `{a2a_exposed: true}`, workspace admins, not an
  `external` agent, audited as `agent_a2a_changed`). Its agent card is served to anyone at
  `GET /.well-known/agent.json?agent={principal}` and `GET /a2a/agents/{principal}/card` (the
  principal percent-encoded, or the id; an agent that is not exposed is not found): the name,
  the purpose as the description, the JSON-RPC endpoint under `NEXC_PUBLIC_URL`, the workspace
  as the provider, `streaming: true`, bearer agent API tokens as the authentication, `text` and
  `file` as input and output modes, and skills from the role template (with its competency
  questions as examples) and from every tool of the resolved capabilities. `POST
  /a2a/agents/{principal}` takes A2A JSON-RPC from a registered counterpart presenting the
  agent API token issued to its principal: `tasks/send` (answered once the run ended, or after
  25 s with the task still working), `tasks/sendSubscribe` (status and artifact events as SSE),
  `tasks/get`, `tasks/cancel`, `tasks/resubscribe`; task ids are UUID v7 (minted when the
  caller sends none). A task is a one-node run of the exposed agent in a managed graph of its
  own (`graphs.a2a_agent_id`, the agent pinned to every node): the message's text parts are the
  node's content, file parts become documents of the knowledge base (the documents' default
  label) that the node is told about, and the run's output and artifacts come back as A2A
  artifacts (the answer as text, each file inline up to 4 MiB, else by a URI the caller fetches
  with its token at `GET /a2a/tasks/{tid}/artifacts/{aid}`; a file the DLP classifier kept in
  is not given out). The run is executed by the instance that took the task
  (`scheduler::execute_claimed`) and its trace opens with the inbound call as a `tool_call`
  span named `a2a:<counterpart>` marked `inbound: true` (the caller's principal, the task id,
  the hop and the chain, a digest of the message) and the firewall's decision as a `policy`
  span (`action: counterpart`, `firewall: true`). A counterpart the agent's `counterparts`
  allow-list does not name, a hop past the delegation depth or a loop (the chain already names
  the agent) is refused: the task is recorded, its run fails at once with the `policy` span's
  denial, and the task is `failed` with the reason. A missing or wrong token answers 401, a
  token of an agent that is no counterpart or is out of service 403, an agent that is not
  exposed or not in the caller's workspace 404, each as a JSON-RPC error (`-32010`, `-32011`,
  `-32012`). `GET /agents/{id}/a2a/tasks` lists an agent's inbound tasks for the console
  (`A2aTask`: who called, the run, the hop, the chain, the refusal). The policy engine gains
  the action `counterpart` (the `counterparts` list and `delegation.max_depth` decide it as for
  a sub-agent), tool lists accept `a2a:<name>` entries, and the metric
  `nexc_a2a_tasks_total{direction, outcome}` counts tasks. In Docker and Kubernetes the
  frontend's nginx, the ingress and the Helm chart proxy `/a2a/` and `/.well-known/agent.json`
  to the backend as they do `/api/` (the Vite dev server too). A workspace transfer copies the
  counterparts without the header sent to them and the inbound tasks with their runs; the
  counterparts' inbound tokens are issued again on the target, as every external agent's are.
  Tests (`backend/tests/a2a.rs`): the
  card at both addresses and nowhere for an agent not exposed, a member refused and an admin
  audited; a counterpart's task runs the Analyst and gets the `.docx` back with the trace
  opening on the inbound call and the admission; streaming, files as documents, cancellation;
  wrong, missing, revoked, foreign and non-counterpart tokens refused with 401, 403 and 404, a
  counterpart off the list refused on the record; the worst-case suite reads the new routes.
  `docs/AGENTSECOPS.md` §11 says how A2A sits on the firewall (a counterpart is a principal, an
  inbound task is a run, an outbound task is an egress, depth and loops cross platforms);
  `docs/ARCHITECTURE.md` and the README name the adapter.
- Alert routing: a workspace's alerts and approvals reach the outside. Admins register
  **alert destinations** (Settings → Alert destinations, `GET/POST
  /workspaces/{wid}/alert-destinations`, `GET/PATCH/DELETE …/{id}`, `POST …/{id}/test`, `GET
  …/{id}/deliveries`): a signed JSON webhook or a Slack incoming webhook, with filters (alert
  kinds, the least severity, approvals too). The URL is kept encrypted under the master key and
  only its host is shown; a private or loopback host is refused as the gateway refuses it
  (`NEXC_ALERT_ALLOW_PRIVATE_HOSTS=true` allows it in development); a signing secret
  (`nexc_whs_…`) is generated, shown once and kept encrypted. Every change is audited as
  `alert_destination_changed`.
- Delivery: when an alert is raised or an approval is requested, decided or expires, a delivery
  per matching destination goes into the `alert_deliveries` outbox; a worker takes due rows with
  `SKIP LOCKED`, posts the JSON envelope (`id` as idempotency id, `event`, `timestamp`, the
  workspace and agent, the alert or approval as the API returns it, links into the console at
  `NEXC_PUBLIC_URL`) with `X-Nexc-Signature: sha256=<HMAC-SHA256 of the body>` and
  `X-Nexc-Timestamp`, or a Block Kit message for Slack (severity colour, title, agent, rule,
  link buttons); 10 s per attempt, retried after 30 s doubling up to an hour for 24 hours, 4xx
  (and redirects, which are not followed) failing for good. Each destination keeps a log with
  status, attempts, the last response code and error. A destination that fails fifty times in a
  row is switched off, audited by `system`, and an alert of kind `policy` (rule
  `alert_destinations.failures`) tells the admins. `Alert.deliveries` counts the destinations an
  alert reached; the Alerts panel on `/app/secops` shows "sent to N destinations". Metric
  `nexc_alert_deliveries_total{kind, outcome}`.
- SIEM pull: `GET /workspaces/{wid}/alerts/export?since=&until=&format=ndjson|json` and `GET
  …/audit/export` stream alerts and audit entries over a window, oldest first, with their stable
  ids, at most 10,000 per pull, 30 pulls a minute per account; the way a collector pulls without
  webhooks (`docs/CONTRACT.md` §5, with a signature verification example in Python under §4).
- `NEXC_PUBLIC_URL` and `NEXC_ALERT_ALLOW_PRIVATE_HOSTS` in `.env.example` and CONTRACT §2.
  A workspace transfer leaves alert destinations and their log behind (their secrets are sealed
  with this server's key); they are registered again on the target.
- The AgentSecOps console (phase 5, `docs/AGENTSECOPS.md` §8): **AgentSecOps** in the sidebar
  (`/app/secops`, members and above) is the one screen a CISO wants: a stat row (agents by
  status, runs today and this week, spans and denials today, open approvals, unacknowledged
  alerts by severity, held artifacts, the last sealed trace), a **Registry** of every agent as a
  row (name, role, owner, team, principal, model, status, risk score with its reasons on hover
  or focus, policy version, heartbeat, budget left) with text and status filters, quarantine and
  release at hand and an agent-from-template action; a **Live feed** of every graph's
  `trace.span` and `alert` events, newest first, filtered by kind and agent, with a pause that
  holds lines back rather than losing them; **Approvals** decided inline (a denial asks its
  reason there); **Alerts** acknowledged inline; **Risk**: a line chart of each agent's score
  over the last fourteen days, the highest-risk agents now and denials by rule; **Policies**:
  the resolved policy of any agent rule by rule and a diff between any two versions of any
  level (phase 2's views, reused); **Ask the audit** embedded (phase 3's panel); and a second
  tab, the **Trace explorer**: spans across every run, narrowed by agent, kind, decision, tool,
  host, run and time, as a table, paged, a row opening the run's waterfall at that span
  (`/app/runs/{id}?span=`, which marks and scrolls to it). Everything is reachable by keyboard,
  works at phone width and in both themes.
- `GET /workspaces/{wid}/secops/overview` (`SecopsOverview`, one call for the top of the page;
  runs, spans, denials and artifacts count only what the caller may open), `GET
  …/secops/spans?agent=&kind=&tool=&host=&run=&from=&to=&decision=&limit=&before=` (the explorer's
  search: ids plus the attributes the table shows, newest first, paged) and `GET …/secops/events`
  (the workspace's `trace.span` and `alert` events across every graph the member may open as one
  bearer-authenticated SSE feed, read with `fetch`; graphs created meanwhile are picked up within
  ten seconds). Every risk score an agent is given is kept in `agent_risk_history` by a trigger
  on `agents.risk_updated_at`, so the chart has fourteen days without the engine knowing.
- The agent card (from the registry and `/app/agents?agent=`) has tabs: **Overview** (purpose,
  owner, team, scopes, model, budget, risk, policy version, evaluation score; for an agent of
  runtime `external`, its **API tokens**: each with its name, prefix, when it was issued, last
  used and revoked, an Issue form whose token is shown once with a copy and a Revoke behind a
  confirmation, workspace admins only, and the list is never copied to browser storage),
  **Activity** (its
  last spans, each opening its run at the span), **Policy** (the resolved policy with the level
  that set each rule), **Approvals** and **Alerts** about that agent, decided and acknowledged
  there, and **Ask** with the agent filled in.
- Role templates (§4.6): **Analyst**, **Researcher** and **Reviewer** ship as data in
  `backend/templates/agent-*.json` with capabilities, a policy, knowledge bindings and three
  competency questions each (`GET /agent-templates`). `POST /agents/from-template/{name}` makes
  the agent with the template's scopes, its model allow-list pinned to the model it is given,
  the workspace's matching knowledge topics bound, the template's policy as the agent's version 1
  (audited), and asks the questions once with the workspace's provider (usage purpose
  `evaluation`); the score and the answers are kept (`agent_evaluations`) and shown on the card
  (`AgentCard.evaluation`). The console's registry and the demo make agents this way.
- `make demo-secops` runs `scripts/demo-secops.py`, the five-minute script of
  `docs/AGENTSECOPS.md` §9, on the demo provider: an Analyst from the template with its score,
  a traced run, the policy changed to require an approval for `make_docx` and the approval
  decided from the inbox, a restricted document withheld, a call to a host outside the
  allow-list through the gateway, the agent quarantined with one call while a run is going,
  the six audit questions, the prove targets (`make prove-agentsecops`: identity, policy,
  audit and trace in one) and the run's chain verified; every step prints the URLs to open;
  re-runnable, and it removes what it made unless `--keep` (`DEMO_ARGS="--keep --skip-prove"`).
  The disallowed host is real: the script starts an HTTP stub on the machine
  (`NEXC_DEMO_STUB_HOST`, `127.0.0.1` for a local backend, `host.docker.internal` for the
  compose stack, which `make demo-secops` sets and `docker-compose.demo.yml` resolves;
  `NEXC_DEMO_STUB_PORT` fixes the port), tells the research node to read the figures from it,
  and the agent's `fetch_url` goes through the tools gateway: once with the host named (the
  `egress` span with the host, the status and the bytes; the stub records the gateway's own
  user agent) and once with the host off the list (refused before any connection, the `policy`
  span with the rule, the alert). On the NDJSON wire no gateway tool is offered, so the script
  falls back to a tool the policy does not allow and prints which variant ran. The demo
  provider reads the http(s) URLs a node's brief names (up to three) through `fetch_url` when
  the gateway offers it, and lists them under the research notes' sources; it still never
  touches the network itself. Run on the compose stack (`docker compose -f docker-compose.yml
  -f docker-compose.demo.yml up -d --build --wait`, then `scripts/smoke.sh` passed, then `make
  demo-secops`), it printed: step 1 `competency questions: 1 / 3`; step 2 `succeeded: 8 spans
  (2 data_read, 4 llm_call, 2 tool_call), policy mismatches 0`; step 3 `node … waits: the
  approval is in the inbox`, `approved make_docx by Sam Secops; the run goes on`,
  `succeeded: 10 spans, 1 approval span(s) on the trace`; step 4 `document
  board-compensation.md: ready, restricted, 1 passages`, `a local HTTP stub started by this
  script answers http://host.docker.internal:56741/quarterly-figures.json`, `succeeded: 1
  fetch_url call(s) through the gateway, 1 egress span(s)`, `egress host.docker.internal HTTP
  200, 86 bytes in`, `the stub answered 1 request(s), from nexc-engine-gateway/0.1.0`, then
  with the allow-list empty `succeeded: 1 egress denial(s) on the trace, the stub got 0
  request(s)`, `variant: a host outside the allow-list, refused by the gateway`, `denied egress
  host.docker.internal by egress.hosts: host host.docker.internal is not allowed`, `alert
  [medium] risk alerts.denials_per_run: 3 policy denials in one run`, `alert [high] dlp
  egress.hosts: fetch_url refused for host.docker.internal`, `quarantined Analyst (demo)
  (policy version 3)`, `run …: failed, 1 node(s) stopped`; step 5 the six questions answered
  `onto-intelli is not wired (make demo-onto)`; step 6 `chain intact, 14 spans`, `overview: 6
  agents (1 quarantined), 5 runs today, 59 spans, 9 denials, 1 approvals open, 5 alerts open`,
  then the prove targets: identity p99 0.25 µs (backend) and 1.71 µs (runtime), policy
  resolution p99 6.3 µs and decisions p99 0.21 µs (backend) and 2.00 µs (runtime), audit asks
  p99 3.5–6.9 ms in process and 5.0 ms over gRPC, trace fan-out p99 25 µs over 10,000 spans,
  durable 389.8 ms after the last emit, verify 45.1 ms (the chain alone 8.8 ms).
- Tests (`backend/tests/secops.rs`, `frontend/src/features/secops/`): the overview counts the
  workspace's agents, runs, spans, denials, approvals, alerts, risk and seal and nothing a
  guest or an outsider may see; the explorer narrows by every filter, pages without repeats,
  refuses bad input and clamps the limit; the live feed carries a run's spans as they happen and
  nothing else, and refuses guests and outsiders; an agent made from a template arrives with its
  scopes, its pinned model, its policy version, its audited change, its scored questions and
  their usage, and conflicts, unknown templates, guests and bad bodies are refused; the feed's
  frames become lines once each, the risk history lays out day by day, the registry filters.
- gRPC between backend and runtime (AgentSecOps phase 4, `docs/AGENTSECOPS.md`): the
  vendored `backend/proto/nexc/runtime/v1/runtime.proto` defines `AgentRuntime` with
  `Execute(stream BackendCommand) returns (stream RuntimeEvent)`, one stream per node attempt
  that the backend opens; the request is the first command, the events are the NDJSON lines as
  messages, and `ack`, `cancel`, `approval_decision`, `policy_update` and `tool_result` go down
  the same stream, so an approval reaches the replica holding the call and a policy change
  reaches a running agent as a fresh token. The runtime serves it with grpclib on
  `RUNTIME_GRPC_PORT` (8095) beside FastAPI, reusing the runner, the event stream and the
  approvals (`make proto-runtime` regenerates its stubs); the backend's tonic client is chosen
  by `NEXC_RUNTIME_PROTOCOL` (`grpc`, the default, or `ndjson`, kept for one release) and
  `NEXC_RUNTIME_GRPC_URL`. The backend is the client of the stream, not the runtime as the plan's
  text had it, so the runtime remains reachable from the backend only.
- Tools gateway: MCP servers are registered per workspace (`GET/POST/DELETE
  /workspaces/{wid}/mcp-servers`, `…/check`; stdio or streamable HTTP, an encrypted
  `Authorization` header, an allow-list of tools, a data clearance `max_label`; registering
  connects and records the tools listed) and offered to agents as `mcp:<server>/<tool>` under
  the policy (`mcp:<server>/*` in a tool list names them all), with the built-in `fetch_url`.
  The runtime sends `tool_request` up the stream and the backend performs the call: the rate
  limit per agent (`NEXC_GATEWAY_CALLS_PER_MINUTE`, the existing token bucket), the policy on
  the tool and on the host (private and loopback addresses refused as `egress.private` unless
  named, redirect hops checked one by one), the DLP screen of the arguments under the context's
  labels, the timeout (`NEXC_GATEWAY_TIMEOUT_SECS`) and the size cap
  (`NEXC_GATEWAY_MAX_RESPONSE_BYTES`), recorded as a `tool_call` and an `egress` span. A stdio
  server runs with an empty environment in a scratch directory for the one call. Settings → MCP
  servers registers, checks and removes them.
- Agent Firewall API: `backend/proto/nexc/firewall/v1/firewall.proto` defines `AgentFirewall`
  (`AuthorizeAction` answering a decision signed with Ed25519, `ReportSpan` chaining an external
  agent's spans into a verifiable run of its own and re-deciding each of them, `VerifyTrace`,
  `GetAgentCard`), served by tonic on `NEXC_GRPC_ADDR` with reflection and `grpc.health.v1`,
  off when unset. Agents of the new runtime `external` hold API tokens (`POST
  /agents/{id}/tokens`, shown once, stored as a SHA-256 hash, revocable, audited as
  `agent_token_issued` / `agent_token_revoked`); the token and the agent's status are read on
  every call, so a quarantine or a revocation ends access at once. `NEXC_FIREWALL_SIGNING_KEY`
  sets the signing seed (derived from the master key when unset); the public key is on
  `/admin/infrastructure`.
- OpenTelemetry export: with `NEXC_OTLP_ENDPOINT` set, a worker sends every span of the trace
  to the collector over OTLP/gRPC (the run is the trace, the span id the last 8 bytes of the
  UUID v7, the parent kept, the workspace the resource, attributes as stored) in batches behind
  a shared cursor (`otlp_export_cursor`). `nexc doctor` and Settings → Infrastructure report the
  runtime stream, the gateway's limits, the firewall and the collector.
- `make prove-gateway` measures the gateway's cost per call (the tool excluded) and the stream's
  per-event latency over gRPC and NDJSON; in-process tonic tests cover the stream (a run with
  its spans live and its chain sealed, cancellation, an approval asked and decided on the
  stream, a dropped stream, a gateway request performed and one refused), the gateway
  (registration, a stdio and an HTTP MCP server, the allow-lists and the rate limit, labels,
  deny-lists and personal data in arguments), the firewall (signed decisions and a verifiable
  run, wrong tokens and workspace isolation, reflection and health) and the export (a collector
  stub); the runtime's tests cover its grpclib service, policy updates included; the worst-case
  suite reads the new routes from the spec. Recorded on an Apple-silicon release build: gateway overhead per call,
  500 calls each: an MCP tool over HTTP p50 8 µs, p99 17 µs, max 833 µs (the whole call p50
  133 µs, p99 387 µs); `fetch_url` p50 7 µs, p99 13 µs, max 17 µs (the whole call p50 53 µs,
  p99 147 µs). Per-event latency from the runtime's send to the backend's fan-out, 2000 deltas:
  gRPC p50 56 µs, p99 117 µs, max 167 µs; NDJSON p50 63 µs, p99 128 µs, max 161 µs.
- A Helm chart, `deploy/helm/nexc-engine`: the objects of `deploy/k8s` templated, with the
  images and their tag, the replicas, the non-secret settings, the runtime's HTTP and gRPC
  ports (`runtime.grpcPort`, carried into the backend's `NEXC_RUNTIME_GRPC_URL`, the runtime's
  Service and the NetworkPolicy), the Agent Firewall API (`backend.grpcAddr` sets
  `NEXC_GRPC_ADDR` and opens its Service port to the namespaces of
  `networkPolicies.firewallFrom`), `backend.otlpEndpoint` (`NEXC_OTLP_ENDPOINT`) and
  `backend.ontoUrl` (`NEXC_ONTO_URL`), the ingress host and TLS, PostgreSQL in the chart or
  brought along, and the secrets from an existing Secret or from values (the chart refuses to
  render while a required one is empty). CI lints the chart and renders it with the firewall,
  the collector and onto-intelli on; `deploy/k8s/README.md` has the install.
- Operations: `.env.example`, `docker-compose.yml` (the runtime's gRPC port on the internal
  network only), the Kubernetes manifests (the runtime's Service and NetworkPolicy admit the
  backend on 8095; the firewall port is documented for when it is opened to another namespace),
  `docs/CONTRACT.md` §1, §2, §4, §5 and §8 and `docs/ARCHITECTURE.md` describe the wire, the
  gateway, the firewall and the export. A workspace transfer copies the MCP servers without
  their `Authorization` headers and leaves the API tokens of external agents behind, both to
  be entered again on the target, as it does with the other stored credentials.
- Ask the audit (AgentSecOps phase 3, `docs/AGENTSECOPS.md` §4.5): the trace becomes ontology
  facts in [onto-intelli](https://github.com/f8fvwgzc/onto-intelli) and audit questions are answered
  from them. `NEXC_ONTO_URL` (its gRPC endpoint; unset keeps the feature off), `NEXC_ONTO_TOKEN`
  (generated by `make init`) and `NEXC_ONTO_WORKSPACE_PREFIX` (default `nexc-`) configure it;
  `nexc doctor` and Settings → Infrastructure (`GET /admin/infrastructure`, `ontology`) report
  whether it answers, its version and the model it explains with. The client is tonic, compiled
  from the vendored `backend/proto/onto_intelli/v1/onto_intelli.proto` with protox (no `protoc`),
  behind the trait `ontology::OntologyBackend`; an in-memory stub, also served as an in-process
  gRPC server, stands in for tests.
- Fact export: the tables queue what they write (`onto_export_queue`, filled by triggers on
  `agent_spans`, on `runs` when they finish and on `agents` when their policy changes) and a worker
  turns it into entities, links and events, one onto-intelli workspace per nexc workspace:
  `agent —ran→ run`, `run —executed→ node`, `agent —used_tool→ tool`, `agent —used_model→ model`,
  `agent —read→ document | memory` (passage ids resolved to documents, with their classification),
  `agent —called→ host`, `agent —delegated_to→ agent`, `policy —denied→ action`,
  `action —denied_in→ run`, `agent —governed_by→ policy`, `policy —allows→ tool`, with the run's
  status, cost and tokens as properties and an event per action (`agent.read`, `agent.tool`,
  `policy.denied`, `run.finished`, …). Only ids, names, hosts, counts, decisions, labels, status
  and cost leave the database: a span's attributes are read for the few keys that name things,
  never copied, and no prompt, argument, output or document content is ever a fact. Rows are
  claimed with `SKIP LOCKED` and deleted once onto-intelli acknowledged the stream (at least once;
  instances share the work); a workspace seen for the first time is backfilled from what it has,
  up to the first queued span (`onto_export_cursors`, with each workspace's last error).
- `POST /workspaces/{wid}/audit/ask` `{question}` → `AuditAnswer`: the answer, how it was produced
  (`deterministic`: from the graph alone, 0 tokens; `llm`: a model explained with the facts it was
  given), the facts it rests on as things in nexc with links (the agent's card, the run, the
  document, the policy), the tokens and the latency; an answer that cites no fact is
  `unverified`. Six canned questions (`GET …/audit/questions`) are recognised with their slots and
  answered as graph and event queries: which agents read restricted data this week, which agents
  called hosts outside the allow-list, why was run X denied, what did agent Y do today, which
  policies allow code execution, who approved what this month. Anything else, and every "why" or
  "explain", goes to onto-intelli's `IntelliService/Ask`; `POST …/audit/ask/stream` streams its
  explanation as server-sent events (`stage`, `delta`, `done`, `error`). Members and above may
  ask; the asker's workspace role is their clearance (owners and admins see every fact, members do
  not see confidential or restricted data ids: the fact is withheld and the id replaced). 503
  problem+json when the feature is off or onto-intelli does not answer; 403 when it refuses the
  principal; 422 for an empty or oversized question. Metric `nexc_audit_asks_total` by mode.
- The Audit page and the agent card have an **Ask the audit** panel: the six questions as chips
  (the card fills in its agent), a question box, the answer with its mode (deterministic in green
  with 0 tokens and the latency, llm neutral), the cited facts as links, and an "Unverified" mark
  when it cites none; `/app/agents?agent=` opens an agent's card from an answer.
- `docker-compose.onto.yml` (profile `onto`) builds onto-intelli from a checkout next to this
  repository and wires the backend to it (`deploy/onto/onto-intelli.toml` with token auth and the
  audit's schema `deploy/onto/nexc-audit.toml`); `make demo-onto` brings the demo stack up with it,
  runs `scripts/demo-onto.py` (three agents with different clearances, a restricted document, a
  run that reads it) and asks the six questions, printing mode, tokens, latency and facts.
- Tests (`backend/tests/audit.rs`, `frontend/src/features/audit/`): every span kind becomes its
  facts and the cursor moves; the export resumes after onto-intelli was down and after a lost
  cursor without doubling the graph; nothing but ids, names, hosts, counts and decisions leaves
  (poisoned attributes, node titles, document names, passages, memories and prompts never do);
  the six questions are answered from the graph with links and a member's clearance withholds the
  restricted document; free questions reach onto-intelli, their facts resolve and an answer with
  claims and no facts is unverified; the endpoint fails safely (off → 503 naming `NEXC_ONTO_URL`,
  onto-intelli down → 503, permission denied → 403, oversized or empty → 422, guests 403,
  outsiders 404); the tonic client speaks to the in-process server end to end, the wrong token is
  refused and a stopped server is a 503; the panel marks an unverified answer and links the facts.
  `make prove-audit` measures the ask end to end through the HTTP router with the stub answering,
  2,000 asks per question, 256 askers (target p99 < 5 ms in process). Apple M2 Pro, release
  build, PostgreSQL 17 in Docker:
  - in process (the backend's own cost: token check, membership, the queries, the facts resolved
    against PostgreSQL): restricted reads p50 2.1 ms, p99 4.2 ms; why was run X denied p50 2.1 ms,
    p99 4.0 ms; which policies allow code execution p50 1.4 ms, p99 3.2 ms; what did agent Y do
    today p50 3.6 ms, p99 8.8 ms (two agents of that name, so twice the event queries, plus the
    label lookups of what it read);
  - over gRPC to the in-process server: restricted reads p50 2.3 ms, p99 5.1 ms (the hop adds
    about 0.3 ms at the median).
  - A real onto-intelli 0.2.0 (release build, SQLite, mock model) on loopback, the backend in
    debug, through `scripts/demo-onto.py`: the six questions answer deterministically with 0
    tokens in 0.4–6.0 ms each (restricted reads 3.7–4.5 ms, hosts outside the allow-list
    0.5–1.3 ms, why was run X denied 1.6–4.9 ms, what did agent Y do today 4.0–6.0 ms, policies
    allowing code execution 2.1–5.1 ms, approvals 0.4–0.9 ms), with the agent ids and the document
    linked into nexc; onto-intelli's own deterministic "how many runs" answers in 6 ms.
- onto-intelli reads an ingest stream as `google.protobuf.Struct`, whose numbers are doubles, so
  counts in the audit's schema are typed as floats and events carry the moment they happened as
  `payload.at` rather than `ts` (its `Append`, which keeps integers, only reads back the newest 64
  events); the time-windowed questions narrow by `at`.
- Versioned policies (AgentSecOps phase 2, `docs/AGENTSECOPS.md` §4.3 and §4.4): rules are data,
  kept as an append-only history per level in `policies` (`workspace`, `team`, `agent`; each
  version has an author and a time). A `PolicyDocument` holds tool, host and model allow- and
  deny-lists, the clearance labels, token and cost ceilings per run and per day, tool calls per
  minute and node starts per hour, time windows (UTC), which tools and labels require an
  approval and who approves, alert and quarantine thresholds, the DLP rules (secrets, PII
  shapes and keywords, where content of each label may go) and the prompt-injection screen's
  action. Resolution walks workspace → team → agent over the agent's own capabilities and only
  narrows: allow-lists are met, deny-lists added, labels intersected, limits lowered, approvals
  and thresholds added, actions tightened; `GET /workspaces/{wid}/policies/resolve?agent_id=`
  shows the effective policy and which level set each rule. `GET/POST
  /workspaces/{wid}/policies` list the history and add a version (admins); a version that would
  widen the levels above is refused rule by rule, and every version is audited as
  `policy_changed` with what changed since the previous one. The resolved policy is compiled
  into the per-run token (`capabilities` narrowed, plus `rules`, contract §8) and the versions
  in force are recorded on the run (`policy_versions`), on each node run (with the
  `context_labels` of the data it was given) and on every policy span. Before dispatch the
  backend refuses a node outside its time windows, over the day's token or dollar ceiling or
  past the agent's starts per hour (a `policy` span of rule `time_windows`,
  `limits.tokens_per_day`, `limits.cost_usd_per_day` or `limits.runs_per_hour`); the ceiling per
  run caps the budget the runtime is given.
- Decisions beyond allow and deny: a `Decision` now carries an effect, `allow`, `deny`,
  `require_approval` (with the approvers) or `alert` (with a severity). The runtime decides every
  tool call from the token's rules as well as its capabilities: a deny-list refuses first
  (`tools.deny`, `models.deny`), then the capabilities, then an approval rule holds the call.
  A held call (`require_approval.tools`, or `write_file` and `make_docx` after data of a label in
  `require_approval.labels`) is reported as a `policy` span with decision `require_approval` and
  waits; the backend records the request (`approvals`), sets the node to the new status
  `waiting_approval` while the run stays `running`, asks the approvers in their inbox (the new
  notification kind `approval`; every admin unless the policy names members) and, once one of
  them decides with `POST /approvals/{id}/approve` or `/deny` (a reason is required to deny),
  hands the decision to the runtime (`POST /v1/approvals/{id}`, contract §8), records it as an
  `approval` span naming who decided and why, audits it (`approval_granted`, `approval_denied`)
  and sets the node back to `running`. Approved, the tool runs with its approval on the span;
  denied, the model sees the reason as a tool error and goes on; undecided within the policy's
  `timeout_s` (default 600 s), the node fails and the approval is `expired`. `GET /approvals/{id}`
  and `GET /workspaces/{wid}/approvals` show them. The runtime also refuses tool calls past
  `limits.tool_calls_per_minute` and keeps the automatic Markdown-to-.docx fallback from making
  what a policy holds or denies.
- DLP on the way out: every artifact a node delivers is classified (credentials with the
  guardrails' scrubber; e-mail addresses, phone numbers, card numbers and national-id shapes by
  hand; the workspace's own keywords; the words of Office documents are read out of their XML)
  and checked against the strictest classification the node read, its own context's and its
  upstream nodes' (`context_labels` on the node run). Content derived from `restricted` data
  never leaves as an artifact and `confidential` content never leaves to a host unless the
  policy opens it (`dlp.labels`); a credential keeps the file in by default (`dlp.secrets`),
  personal data raises an alert by default or keeps the file in under `pii.action: deny`. A
  quarantined artifact is stored but answers 403 to downloads and stays out of the zip until an
  admin releases it (`POST /artifacts/{aid}/release`, audited as `artifact_released`;
  `Artifact.quarantined_at`, `quarantine_reason`, `released_at`). A tool that calls out reports
  `egress_host` on its span and the backend checks the host and the labels (`egress.deny`,
  `dlp.labels`). Every finding is a `policy` span (decision `deny` or `alert`, counts and digests
  only) and an alert: the new `alerts` table, `GET /workspaces/{wid}/alerts`, `POST
  /alerts/{id}/acknowledge`, the inbox kind `alert` for every admin, the SSE event `alert` on the
  graph, the metrics `nexc_alerts_total{kind}` and `nexc_artifacts_quarantined_total`, and the
  audit actions `artifact_quarantined`, `artifact_released` and `alert_raised`.
- Behaviour baseline and risk score: `agent_baselines` keeps, per agent, what one of its runs
  usually looks like (tool calls per turn, tokens per turn, distinct tools, distinct hosts,
  sub-agents, denials, bytes out, as running means and deviations) and the MinHash signatures
  (the C kernel) of its last twenty tool sequences. When a run finishes every agent that worked
  in it is scored first and learns the run after: the capped sum of the z-scores of what strayed,
  plus hosts and tools it never used and a tool sequence unlike any earlier run, explained in
  words on the agent (`risk_score`, `risk_reasons`: "4.8x its usual bytes out", "2 hosts it never
  called"). The policy's `alerts` thresholds raise alerts of kind `risk` and, at
  `quarantine_risk_score`, quarantine the agent automatically (its running nodes are cancelled,
  new ones refused, audited as `agent_quarantined` with the score and the reasons).
- Prompt-injection screen: the backend screens every upstream output, memory and document passage
  as it builds a node's context and the runtime screens every tool result before it goes to the
  model, with the same cheap pattern set (ignored instructions, role overrides, prompt-leak and
  exfiltration requests, tool hijacking, chat-template markup, hidden zero-width text) and an
  optional confirmation by the run's model (`injection.model_check` in the policy, behind
  `RUNTIME_INJECTION_MODEL_CHECK`). A finding is a `policy` span named `injection` with its
  severity and patterns; by default the text goes through (`flag`), under `injection.action:
  block` it is withheld and the model told why; a high finding is an alert of kind `injection`.
- Console: a **Policies** page under Settings (admins) with the version history of each level,
  the diff against the version before, a JSON editor for the next version and the effective
  policy of any agent rule by rule with the level that set each; approvals in the **Inbox** with
  Approve and Deny (with a reason); an **Alerts** feed under Settings with acknowledgement; the
  risk score and its reasons on the agent card; the run page shows a node waiting for an
  approval, the decision and who took it in the trace, and quarantined artifacts with Release
  for admins; the audit log reads the new actions.
- Tests (`backend/tests/policy.rs`, `agent-runtime/tests/test_policy.py`,
  `agent-runtime/tests/test_injection.py`): policies are versioned, audited with their diff and
  refused when they widen the level above; the resolved policy reaches the token and its versions
  the run, the node run and the spans; working hours, the day's spend and the pace stop a node
  before dispatch; a policy that requires an approval pauses a run until an admin approves from
  the inbox, and a denial reaches the model with its reason; a restricted-derived artifact is
  quarantined until an admin releases it, a credential keeps a file in, personal data raises an
  alert, confidential content stays off the wire; a run five times louder than usual raises the
  risk score and the threshold quarantines the agent; a planted instruction in a tool result or
  an upstream output is flagged, and withheld when the policy blocks. `make prove-policy` prints
  the resolution and decision latencies (targets: resolution under 1 ms, decision p99 under
  100 µs). Apple M2 Pro, release build:
  - resolution of three levels over the agent's capabilities, 10,000 rounds: p50 3.0 µs,
    p99 6.4 µs, max 264 µs;
  - backend decisions under the resolved policy (`decide_with`, a quarter denied, a quarter held
    for approval), 100,000 of them: p50 0.08 µs, p99 0.25 µs, max 792 µs;
  - runtime decisions under rules (`decide_tool_with`), 100,000 of them: p50 0.58 µs,
    p99 1.46 µs, max 44.9 µs.
- Agent identity and scopes (AgentSecOps phase 1, `docs/AGENTSECOPS.md` §4.1): every agent is a
  principal, `agent://{workspace}/{name}` (`principal_id`, unique, following the name), with an
  owner (`owner_user_id`), a team, a `purpose` and a `capabilities` document: the tools it may
  call, its data scopes (knowledge topics, the classifications it is cleared for, memory
  namespaces), egress hosts, counterpart roles, delegation depth and fan-out, and a model
  allow-list. `null` on a list means no restriction; `{}` is the default policy (every tool,
  `public` and `internal` data, two levels of delegation with four children each, any model).
  Every change to the capabilities raises the agent's `policy_version` and is audited
  (`policy_changed`). A capability set is a lattice: narrowing can only remove, never add.
- A per-run agent token: for every node it dispatches to the Python runtime the backend mints an
  HS256 token (its own key, `NEXC_AGENT_TOKEN_KEY`, derived from `NEXC_RUNTIME_TOKEN` when unset,
  audience `nexc-runtime`) carrying the principal, the run and node, the capabilities snapshot and
  the policy version, valid for the node's timeout plus a minute (contract §8, `token`). The
  runtime refuses to start without a valid one (signature checked in constant time, expiry,
  audience, run and node), decides every tool call and spawn from it in-process, refuses what
  the policy forbids with a `policy` span naming the rule (the model sees the refusal as a tool
  error and goes on), and gives sub-agents a narrowed copy that is never wider than the parent's.
  The built-in executor and the runtime attribute every span to the principal.
- Data scopes in the backend: documents, knowledge topics and memories carry a classification
  (`label`; `classification` on topics: `public`, `internal`, `confidential`, `restricted`;
  `PATCH /workspaces/{wid}/documents/{did}`, `PATCH /workspaces/{wid}/knowledge/topics/{tid}`,
  `PATCH /memories/{id}`, admins, audited as `classification_changed`). The context builder
  gives an agent only what its clearance, topic scope and memory namespaces allow, before
  anything reaches the runtime; the `data_read` span lists what was given and a `policy` span
  named `context` lists what was withheld (ids only). The model allow-list is enforced before
  dispatch: a node whose model the agent may not use fails without a call, with the reason.
- The backend re-checks every span the runtime reports against the capabilities it minted: a tool
  outside the allow-list, a spawn the delegation rules refuse, a decision the backend would not
  take, a span acting as a principal other than the token's (attributed to the token's, the
  reported one kept as `reported_principal`). Each disagreement is a `policy` span named
  `policy_mismatch`, a `policy_mismatch` entry in the workspace's audit log, a count on the run
  (`Run.policy_mismatches`) and the metric `nexc_policy_mismatches_total`.
- Quarantine: `POST /agents/{id}/quarantine` and `POST /agents/{id}/release` (workspace admins,
  audited as `agent_quarantined` and `agent_released`). A quarantined agent's running nodes are
  cancelled at once on the instance that quarantined it and within a second elsewhere (the node
  fails with a `policy` span, rule `quarantine`), new dispatches are refused, and its policy
  version is raised so no token minted before is honoured. `status` gains `quarantined`.
- `GET /agents/{id}`: the agent card: the agent with its principal, purpose, capabilities and
  policy version, who answers for it, its team, the budget left and its last twenty spans. The
  Agents page opens it in a side panel with quarantine and release for admins; the agent form
  has a capabilities editor; the run page shows a run's policy mismatches next to its trace and
  the waterfall names agents by their principal.
- Tests (`backend/tests/identity.rs`, `agent-runtime/tests/test_policy.py`): the token verifies
  and carries the capabilities, tampered, foreign, expired and misdirected tokens are refused and
  the runtime starts nothing without one; a tool outside the allow-list is refused and recorded;
  a sub-agent cannot exceed its parent's scopes; the backend re-check audits and counts every
  disagreement; a `restricted` document never reaches an `internal` agent and the withheld span
  says so; the model allow-list stops a node before dispatch; quarantine cancels a running node
  and refuses new ones; capabilities that do not fit are refused. `make prove-identity` prints the
  in-process decision latency over 100,000 decisions (target p99 < 100 µs). Apple M2 Pro,
  release build:
  - backend (Rust, `domain::policy::decide`, 28,571 of them denials): p50 0.04 µs, p99 0.12 µs,
    max 38.7 µs;
  - runtime (Python, `decide_tool` + `decide_spawn` per sample): p50 0.88 µs, p99 1.0 µs,
    max 21.3 µs.
- Agent trace spine (AgentSecOps phase 0, `docs/AGENTSECOPS.md`): every action an agent takes in a
  run is an append-only span in `agent_spans` (UUID v7 ids, parent links, kind `llm_call`,
  `tool_call`, `spawn`, `data_read`, `egress`, `policy` or `approval`). The spans of a run form a
  hash chain, `SHA-256(prev_hash || canonical JSON of the span)` from 32 zero bytes, and the
  audit log gains the action `trace_sealed`.
- The agent runtime reports every model turn (`llm_call`), tool call (`tool_call`: the tool, a
  SHA-256 of its arguments, the result size, the error flag and the duration) and sub-agent birth
  (`spawn`) as a `{"type":"span"}` NDJSON line with a UUID v7 id, its parent and nanosecond
  times (contract §8). Spans never carry arguments, contents or credentials.
- The backend owns the chain: it reads span lines field by field (the runtime is not trusted),
  assigns the run, node and workspace agent, records its own spans for the built-in LLM executor
  (`llm_call`) and for the context given to every node (`data_read`: document chunk ids, memory
  ids, upstream nodes), hashes them in order of receipt with the C SHA-256 kernel, fans each out
  at once as the SSE event `trace.span` and writes them in batches. When a run ends its final
  hash is stored on the run (`trace_hash`) and in the audit log (`trace_sealed`). `llm_usage`
  rows carry the agent, the node and, for the built-in executor, the span that spent the tokens.
  A workspace transfer copies the trace as it was chained, so it verifies on the target too.
- `GET /runs/{rid}/trace` (paged, id order), `GET /runs/{rid}/trace/verify` (recomputes the chain
  and names the first span that does not fit) and `GET /agents/{id}/activity` (an agent's latest
  spans across the runs the caller may open).
- The run page shows the trace as a waterfall, grouped node → agent → turn → tool call, filling
  in live from `trace.span` while the run executes, with the chain's verification next to it
  ("Chain intact" or the first span that does not fit). Each agent card on the Agents page lists
  the agent's last actions.
- Trace tests (`backend/tests/trace.rs`): a 12-turn agent run with 40 tool calls and a sub-agent
  yields a chain that verifies and is sealed in the run and the audit log; changing one byte of a
  stored span makes `verify` name that span; oversized span lines, unknown kinds, kinds only the
  backend may record, repeated ids and spans after the result are contained; the trace is private
  and paged. `make prove-trace` pushes 10,000 spans through one run's chain and prints the
  latencies. Three runs on an Apple M2 Pro (10 cores), release build, PostgreSQL 17 in Docker:
  - span emitted → `trace.span` fan-out: p50 9 µs, p99 28–41 µs, max 0.4–0.9 ms;
  - last span emitted → all 10,000 durable in PostgreSQL: 23–111 ms;
  - `GET /runs/{rid}/trace/verify` on 10,000 spans, loading included: 42–45 ms;
  - recomputing the chain alone (C SHA-256 kernel, hashes checked across cores): 7–12 ms.
- Data transfer moves everything: the workspace's files on disk travel inside the target
  database (`workspace_files`) and are written out by the server that owns it at its next start;
  an optional Redis receives the handover record; **Test connection**
  (`POST /workspaces/{wid}/transfers/check`) reaches both before anything moves. The separate
  `files.zip` download is gone.
- The assistant docks beside the page as a full-height panel (the page keeps the rest and the
  graph canvas re-fits to it), is told which page the member is on, and keeps conversations:
  a **Conversations** page lists them as a timeline, opens one in the panel to continue it
  (`/workspaces/{wid}/assistant/conversations`). Conversations are each member's own, come with
  the account export, and go with the account.
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
- Account self-service on the Profile page (and for platform administrators under Account in
  the console): change your name and your password, see where you are signed in and sign out
  everywhere, download a copy of what the installation holds about you, and delete your
  account (`PATCH /auth/me`, `POST /auth/password`, `GET /auth/sessions`,
  `POST /auth/sessions/end`, `GET /auth/me/export`, `POST /auth/me/delete`). Changing the
  password ends every other session at once.
- Deleting an account empties it instead of removing its row, so a team's shared work is not
  lost with it: workspaces the account is alone in are removed, it leaves the others, and what
  it made there stays under "Deleted account". Its name, address, password, sessions, AI
  account, notifications and personal memories are gone, and the address can register again.
  Audit logs and the platform's activity log keep their entries, with the name and address
  replaced by "Deleted account", so they still say what happened without saying who.
  The only owner of a shared workspace has to hand it over first. Platform administrators can
  erase an account on request from the console (`POST /admin/users/{uid}/erase`); the activity
  log keeps the account's id, not its name.
- Password recovery without e-mail: a platform administrator creates a reset link for an
  account from the console (Accounts → Password reset link…) and hands it over. The link works
  once and for an hour, only a digest of its token is stored, a newer link cancels the older
  one, and using it ends every session of the account. The token is in the link's fragment,
  which browsers do not send to servers (`POST /admin/users/{uid}/password-reset`,
  `POST /auth/password/reset`, page `/reset-password`).
- Security activity on the Profile page: sign-ins and failed sign-ins with the address they
  came from, password changes and resets, signing out everywhere, and what a platform
  administrator did to the account (a reset link created for it, a suspension, a role change;
  never the administrator's private reason). It is how a person notices what they did not do
  themselves. Entries are kept 180 days, are part of the copy of your data, and go with the
  account; at most ten failed sign-ins an hour are written, so guessing at an account cannot
  fill its log (`GET /auth/activity`).
- My issues: a page in the sidebar with what is assigned to you and what you filed, across
  every team of the workspace you can see, with the same list, board and filters as the Issues
  page (`GET /workspaces/{wid}/issues?assignee_id=` and the new `creator_id=`).
- Due dates on issues: set when filing or in the issue's properties, shown in lists and on
  board cards ("Due today", "Due tomorrow", the day, or how many days late while the issue is
  open), and recorded in the issue's timeline when set, moved or removed (`due_date` on
  `Issue`, `due` timeline entries). The demo seed gives four issues due dates.
- Due reminders: when an open issue's due date arrives its assignee gets one entry in their
  inbox ("Your issue is due"); a moved date is reminded of again when it arrives. The New issue
  dialog takes a due date.
- Day summaries are told with what surrounded the day, not only its events: the issues that
  were due that day and the state they are in, cycles that started or ended, and what was
  spent on AI. All of it comes from the workspace; nothing is looked up outside it.
- A worst-case test suite that walks every documented route: no access without a valid token,
  nothing of another workspace for an outsider, no server failure on hostile input, one winner
  in races, capped and fast answers on a 20,000-issue workspace, and the API staying up when
  the agent runtime is down (`backend/tests/worst_case.rs`, described in
  `docs/ARCHITECTURE.md`).
- Two-factor sign-in with an authenticator app (TOTP): set up on the Profile page, eight
  one-time recovery codes, a code step on the sign-in page, and a platform administrator can
  turn a lost factor off. A code works once; wrong codes count as failed sign-ins
  (`/auth/2fa`, `/auth/2fa/setup`, `/auth/2fa/enable`, `/auth/2fa/disable`).
- Backups: `make backup` writes the database and the files on disk to `backups/<time>`,
  `make backup-check` restores the latest into a scratch database to prove it can be read, and
  `make restore FROM=…` restores into an empty installation (`scripts/backup.sh`,
  `scripts/restore.sh`; `NEXC_BACKUP_COMPOSE=1` for the Docker stack).
- `scripts/prove-loop.py` runs real issues through plan, agents and result with the configured
  model and reports each node's outcome, the time and the tokens.
- Search from the command palette (⌘K): a few typed characters find issues (by title or
  identifier), projects, graphs, documents, teams and people in the open workspace, grouped by
  kind, and picking one opens it. One request, and only what the caller may see: each kind goes
  through the same rule as its own list (`GET /workspaces/{wid}/search`).
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
  the access token already in hand stops working at once. An event stream the account has
  open ends at its next event or heartbeat, and an open collaboration socket within five
  seconds. The Members page of every workspace
  the account is in marks it as suspended (`WorkspaceMember.suspended`).
- `make seed-demo` (`scripts/seed-demo.py`): a demo workspace with five accounts in different
  roles, three teams, projects, cycles, issues and comments, created through the API so that
  each account's inbox holds real notifications.
- Workflow editor: the Teams page opens a team's workflow, where team owners and workspace admins
  add, rename, recolour, reorder and remove issue states; other members see it read-only.
- "Your teams" in the sidebar opens a team's issues (`/app/issues?team=<id>`), and `C` on the
  Issues page starts a new issue.

### Fixed

- `make db-up` on a machine whose `.env` names the repo's own pgvector image
  (`NEXC_DB_IMAGE=nexc-postgres-pgvector:17`, built once from
  `deploy/postgres/pgvector.Dockerfile`) failed with docker's "pull access denied" (exit 125)
  when that image was not there: `docker run` tries to pull what it does not have, and the
  image is on no registry. `scripts/db.sh` now builds it from the Dockerfile when it is
  missing, pulls any other image it lacks, and says which to check when neither works.
- CI on `main` is green again: the MCP gateway's stdio transport is boxed so that `Transport`'s
  variants are close in size (clippy's `large_enum_variant` fires on Linux, where a `Child` is
  large, and not on macOS), and `scripts/restore.sh` checks for a backup with an `if` rather
  than `A && B || C` (shellcheck SC2015).
- Costs were overstated for long agent sessions: tokens the provider read from its prompt cache
  were priced as fresh input. They are now reported separately end to end and priced at a tenth,
  which is what they cost. A three-node coding issue that showed $3.99 is nearer $1.
- Through the Claude Code provider, an action the model named wrongly was dropped without a
  word, and the protocol told the model to put "the main deliverable in your final text", so
  coding agents described files they never wrote. Unknown actions are now rejected to the
  model's face on its next turn, and the protocol says a file task delivers a file.
- An agent could end a task that delivers a file by describing a file it never wrote; the
  runtime then turned that description into the "deliverable". `finish` now refuses, on such a
  task, until something exists in the workspace, so the model gets told and goes on to write it;
  and when the fallback still has to convert a text answer, the document says so on its first
  page instead of repeating the claim. Found by running a real issue through the loop.

- Uploading a document failed with a 500 when the documents folder had gone missing since the
  server started; the upload now creates it again.

- The API description referenced a schema it did not define (a path parameter's type), which
  broke generating the client types; a test now checks that every reference resolves.

### Changed

- The per-account API rate limit is a setting, `NEXC_API_CALLS_PER_MINUTE` (600, the burst a
  fifth of it, as before), so a load test or an automation account can be given more on its own
  server.
- A tagged read answers `Cache-Control: private, no-cache` instead of `no-store`; untagged
  answers, streams, downloads, the probes and errors keep `no-store`.
- The Agent Firewall's own five-second cache of resolved policies is replaced by the shared
  hot-read cache, dropped on every replica the moment a policy or the agent is written.
- The project is licensed under the MIT License instead of Apache-2.0. No dependency requires a
  stricter licence: every one is MIT, BSD, ISC, Apache-2.0 or similar, with two npm packages
  under MPL-2.0, which only covers changes to those packages' own files.

- The platform and the workspaces are separate APIs. A platform administrator's token is refused
  (403) on every workspace route, whatever the account is a member of, and is accepted on
  `/auth/*` and `/admin/*` only; before, the separation was in the interface alone. A platform
  administrator created by the server or the command line no longer gets a personal workspace,
  and the only owner of a workspace other people work in cannot be made one until the workspace
  has another owner.
- A workspace keeps at least one owner who can sign in: a suspended owner, or one who now
  administers the platform, no longer counts when the last other owner tries to leave or step
  down, and such an owner can be demoted or removed while another remains.
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
