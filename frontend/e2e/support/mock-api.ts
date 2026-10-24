import type { Page, Route } from '@playwright/test';

import { fixtures } from '../fixtures';

/**
 * `full`: a workspace in use. `empty`: the same workspace before anything was made in it.
 * `loading`: the workspace data never arrives (skeletons). `error`: every workspace call fails.
 */
export type DataMode = 'full' | 'empty' | 'loading' | 'error';

/** What the shell needs to stand up at all: answered in every mode. */
const SHELL = /^\/(auth\/|workspaces$|planner\/)/;

/** Who is signed in: a workspace member, or a platform administrator (the console). */
export type Persona = 'member' | 'admin';

export interface MockOptions {
  mode: DataMode;
  persona?: Persona;
  /** Leave the session anonymous (public pages). */
  signedOut?: boolean;
}

/** What the mock answered, for the audit: every API call and those no handler knew. */
export interface MockLog {
  requests: { method: string; path: string; at: number }[];
  unhandled: string[];
}

interface Ctx {
  url: URL;
  params: Record<string, string>;
  mode: DataMode;
  persona: Persona;
}

/** The JSON body to answer with; `undefined` answers 404, {@link PENDING} leaves the request open. */
type Handler = (ctx: Ctx) => unknown;

export const PENDING = Symbol('pending');
const API = '/api/v1';

const full = <T>(mode: DataMode, value: T, empty: T): T => (mode === 'full' ? value : empty);
const list = <T>(mode: DataMode, value: T[]): T[] => full(mode, value, []);

/** Handlers by `METHOD /path/:param` under `/api/v1`. The first that matches answers. */
const handlers: [string, Handler][] = [
  /* auth and account */
  ['GET /auth/me', () => fixtures.auth.user],
  ['GET /auth/sessions', () => fixtures.accountSessions],
  ['GET /auth/activity', ({ mode }) => list(mode, fixtures.accountActivity)],
  ['GET /auth/2fa', () => fixtures.twoFactor],
  [
    'GET /auth/identities',
    ({ mode, persona }) => {
      const methods = fixtures.signInMethods[persona === 'admin' ? 'admin' : 'user'];
      return full(mode, methods, { ...methods, identities: [] });
    },
  ],
  ['GET /auth/providers', () => fixtures.loginProviders],
  ['POST /auth/logout', () => ({})],

  /* workspaces */
  ['GET /workspaces', () => fixtures.workspaces],
  [
    'GET /workspaces/:wid/members',
    ({ mode }) => full(mode, fixtures.members, fixtures.members.slice(0, 1)),
  ],
  ['GET /workspaces/:wid/invites', ({ mode }) => list(mode, fixtures.invites)],
  ['GET /workspaces/:wid/teams', ({ mode }) => list(mode, fixtures.teams)],
  ['GET /workspaces/:wid/teams/:tid/members', ({ mode }) => list(mode, fixtures.teamMembers)],
  [
    'GET /workspaces/:wid/teams/:tid/states',
    ({ params }) => fixtures.states[params.tid ?? ''] ?? [],
  ],
  [
    'GET /workspaces/:wid/teams/:tid/cycles',
    ({ mode, params }) => list(mode, fixtures.cycles[params.tid ?? ''] ?? []),
  ],
  ['GET /workspaces/:wid/usage', ({ mode }) => full(mode, fixtures.usage, fixtures.usageEmpty)],
  ['GET /workspaces/:wid/audit/questions', () => fixtures.auditQuestions],
  ['GET /workspaces/:wid/audit', ({ mode }) => list(mode, fixtures.audit)],
  ['GET /workspaces/:wid/guardrails', () => fixtures.guardrails],
  ['GET /workspaces/:wid/llm', () => fixtures.llmSettings],
  [
    'GET /workspaces/:wid/assistant/conversations',
    ({ mode }) => list(mode, fixtures.conversations),
  ],
  ['GET /workspaces/:wid/assistant/conversations/:cid', () => fixtures.conversation],
  ['GET /workspaces/:wid/search', ({ mode }) => list(mode, fixtures.search)],
  ['GET /workspaces/:wid/transfers', ({ mode }) => list(mode, fixtures.transfers)],

  /* issues */
  ['GET /workspaces/:wid/issues', ({ mode, url }) => list(mode, filterIssues(url))],
  ['GET /workspaces/:wid/labels', ({ mode }) => list(mode, fixtures.labels)],
  ['GET /workspaces/:wid/projects', ({ mode }) => list(mode, fixtures.projects)],
  ['GET /workspaces/:wid/inbox', ({ mode }) => list(mode, fixtures.inbox)],
  ['GET /issues/:iid/events', ({ mode }) => list(mode, fixtures.issueEvents)],
  ['GET /issues/:iid', ({ params }) => fixtures.issues.find((i) => i.id === params.iid)],

  /* graphs and runs */
  ['GET /graphs', ({ mode }) => list(mode, fixtures.graphs)],
  ['GET /graphs/:gid', ({ mode, params }) => (mode === 'full' ? graphById(params.gid) : undefined)],
  [
    'GET /graphs/:gid/runs',
    ({ mode, params }) =>
      list(
        mode,
        fixtures.runs.filter((r) => r.graph_id === params.gid),
      ),
  ],
  ['GET /graphs/:gid/suggestions', () => []],
  ['GET /graphs/:gid/events', () => PENDING],
  ['GET /templates', () => fixtures.templates],
  ['GET /planner/status', () => fixtures.planner],
  ['POST /realtime/tickets', () => ({ ticket: 'e2e-ticket', expires_in: 30 })],
  ['GET /runs/:rid', ({ mode, params }) => (mode === 'full' ? runById(params.rid) : undefined)],
  ['GET /runs/:rid/artifacts', ({ mode }) => list(mode, fixtures.artifacts)],
  ['GET /runs/:rid/trace', ({ mode }) => full(mode, fixtures.trace, { spans: [], next: null })],
  ['GET /runs/:rid/trace/verify', () => fixtures.traceVerify],
  ['GET /runs/:rid/sessions', ({ mode }) => (mode === 'full' ? fixtures.session : undefined)],
  ['GET /spans/:sid/payload', () => fixtures.spanPayloads],

  /* agents and AgentSecOps */
  ['GET /agents', ({ mode }) => list(mode, fixtures.agents)],
  ['GET /agents/:aid', () => fixtures.agentCard],
  [
    'GET /agents/:aid/activity',
    ({ mode, url }) =>
      list(mode, fixtures.agentActivity).slice(0, Number(url.searchParams.get('limit') ?? 50)),
  ],
  ['GET /agents/:aid/tokens', ({ mode }) => list(mode, fixtures.agentTokens)],
  ['GET /agents/:aid/a2a/tasks', ({ mode }) => list(mode, fixtures.a2aTasks)],
  ['GET /agent-templates', () => fixtures.agentTemplates],
  // The skill library is a catalogue: there in every data mode, like the templates.
  ['GET /skills', () => fixtures.skills],
  [
    'GET /skills/:sid',
    ({ params }) => {
      const summary = fixtures.skills.find((s) => s.id === params.sid);
      if (!summary) return undefined;
      // Every skill opens with the fixture skill's knowledge, under its own name.
      const { chars: _chars, ...rest } = summary;
      return { ...fixtures.skill, ...rest };
    },
  ],
  [
    'GET /workspaces/:wid/secops/overview',
    ({ mode }) => full(mode, fixtures.secopsOverview, fixtures.secopsOverviewEmpty),
  ],
  [
    'GET /workspaces/:wid/secops/spans',
    ({ mode }) => full(mode, fixtures.secopsSpans, { spans: [], next: null }),
  ],
  ['GET /workspaces/:wid/secops/events', () => PENDING],
  [
    'GET /workspaces/:wid/sessions',
    ({ mode }) => full(mode, fixtures.sessions, { sessions: [], next: null }),
  ],
  ['GET /workspaces/:wid/policies/resolve', () => fixtures.resolvedPolicy],
  ['GET /workspaces/:wid/policies', ({ mode }) => list(mode, fixtures.policies)],
  ['GET /workspaces/:wid/approvals', ({ mode }) => list(mode, fixtures.approvals)],
  ['GET /workspaces/:wid/alerts', ({ mode }) => list(mode, fixtures.alerts)],
  ['GET /workspaces/:wid/mcp-servers', ({ mode }) => list(mode, fixtures.mcpServers)],
  ['GET /workspaces/:wid/a2a-counterparts', ({ mode }) => list(mode, fixtures.a2aCounterparts)],
  ['GET /workspaces/:wid/alert-destinations', ({ mode }) => list(mode, fixtures.alertDestinations)],
  [
    'GET /workspaces/:wid/alert-destinations/:did/deliveries',
    ({ mode }) => list(mode, fixtures.deliveries),
  ],

  /* memory and knowledge */
  ['GET /memories/topics', ({ mode }) => list(mode, fixtures.memoryTopics)],
  ['GET /memories', ({ mode }) => list(mode, fixtures.memories)],
  ['GET /memories/:mid', ({ params }) => fixtures.memories.find((m) => m.id === params.mid)],
  ['GET /workspaces/:wid/documents', ({ mode }) => list(mode, fixtures.documents)],
  ['GET /workspaces/:wid/knowledge/topics', ({ mode }) => list(mode, fixtures.knowledgeTopics)],
  ['GET /workspaces/:wid/knowledge/settings', () => fixtures.knowledgeSettings],
  ['GET /workspaces/:wid/knowledge/search', ({ mode }) => list(mode, fixtures.passages)],

  /* settings */
  ['GET /settings/llm', () => fixtures.llmSettings],
  ['GET /settings/llm/models', () => fixtures.models],

  /* insight */
  ['GET /workspaces/:wid/timeline/days', ({ mode }) => list(mode, fixtures.timelineDays)],
  ['GET /workspaces/:wid/timeline/summary', ({ mode }) => full(mode, fixtures.daySummary, null)],
  ['GET /workspaces/:wid/timeline', ({ mode }) => list(mode, fixtures.timeline)],

  /* the ontology, in onto-intelli */
  [
    'GET /workspaces/:wid/ontology',
    ({ mode }) => full(mode, fixtures.ontology, { ...fixtures.ontology, proposals: [] }),
  ],
  ['POST /workspaces/:wid/ontology/session', () => fixtures.ontologySession],

  /* the platform console */
  [
    'GET /admin/workspaces',
    ({ mode }) => full(mode, fixtures.platformWorkspaces, fixtures.platformWorkspaces.slice(0, 1)),
  ],
  ['GET /admin/workspaces/:wid', () => fixtures.platformWorkspace],
  [
    'GET /admin/users',
    ({ mode }) => full(mode, fixtures.platformUsers, fixtures.platformUsers.slice(-1)),
  ],
  ['GET /admin/events', ({ mode }) => list(mode, fixtures.platformEvents)],
  ['GET /admin/infrastructure', () => fixtures.infrastructure],
  [
    'GET /admin/latency',
    ({ mode }) => full(mode, fixtures.latency, { window_secs: 300, rows: [] }),
  ],
];

function filterIssues(url: URL) {
  const q = url.searchParams;
  return fixtures.issues.filter(
    (i) =>
      (!q.get('team_id') || i.team_id === q.get('team_id')) &&
      (!q.get('project_id') || i.project_id === q.get('project_id')) &&
      (!q.get('cycle_id') || i.cycle?.id === q.get('cycle_id')) &&
      (!q.get('parent_id') || i.parent?.id === q.get('parent_id')) &&
      (!q.get('assignee_id') || i.assignee?.user_id === q.get('assignee_id')) &&
      (!q.get('creator_id') || i.creator_id === q.get('creator_id')) &&
      (!q.get('label_id') || i.labels.some((l) => l.id === q.get('label_id'))) &&
      (q.get('open') !== 'true' || !['completed', 'canceled'].includes(i.state.category)),
  );
}

function graphById(id: string | undefined) {
  const summary = fixtures.graphs.find((g) => g.id === id);
  if (!summary) return undefined;
  // Every graph opens as the fixture graph, under its own id and name.
  return {
    ...fixtures.graph,
    id: summary.id,
    name: summary.name,
    nodes: fixtures.graph.nodes.map((n) => ({ ...n, graph_id: summary.id })),
    edges: fixtures.graph.edges.map((e) => ({ ...e, graph_id: summary.id })),
  };
}

function runById(id: string | undefined) {
  return fixtures.runs.find((r) => r.id === id);
}

const compiled = handlers.map(([key, handler]) => {
  const [method, path] = key.split(' ') as [string, string];
  const names: string[] = [];
  const pattern = path.replace(/:(\w+)/g, (_, name: string) => {
    names.push(name);
    return '([^/]+)';
  });
  return { method, regex: new RegExp(`^${pattern}$`), names, handler };
});

/** The stand-in for onto-intelli's UI: `onto:ready`, then what the console handed over. */
const ONTO_UI = `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>onto-intelli</title>
<style>body{font:14px system-ui,sans-serif;margin:0;padding:24px;color:#111;background:#fff}</style></head>
<body><main><h1 style="font-size:16px">onto-intelli map</h1><p id="state">Waiting for a session…</p></main>
<script>
addEventListener('message', (e) => {
  if (e.data && e.data.type === 'onto:session')
    document.getElementById('state').textContent = 'Signed in to ' + e.data.workspace;
});
parent.postMessage({ type: 'onto:ready' }, '*');
</script></body></html>`;

function problem(status: number, title: string) {
  return { type: 'about:blank', title, status, detail: null };
}

/**
 * Answers every `/api/v1` call from the JSON fixtures, signs the browser in with a mocked access
 * token (the boot-time refresh is answered), and keeps the graph's realtime channels quiet. No
 * backend is needed.
 */
export async function mockApi(page: Page, options: MockOptions): Promise<MockLog> {
  const log: MockLog = { requests: [], unhandled: [] };
  const user = options.persona === 'admin' ? fixtures.auth.admin : fixtures.auth.user;

  await page.route(
    (url) => url.pathname.startsWith(`${API}/`),
    async (route: Route) => {
      const request = route.request();
      const url = new URL(request.url());
      const path = url.pathname.slice(API.length);
      const method = request.method();
      log.requests.push({ method, path: `${path}${url.search}`, at: Date.now() });

      if (method === 'POST' && path === '/auth/refresh') {
        if (options.signedOut)
          return route.fulfill({ status: 401, json: problem(401, 'Unauthorized') });
        return route.fulfill({ json: { user, access_token: 'e2e-mock-token', expires_in: 3600 } });
      }
      if (method === 'GET' && path === '/auth/me') return route.fulfill({ json: user });

      if (!SHELL.test(path) && options.mode === 'loading') return;
      if (!SHELL.test(path) && options.mode === 'error') {
        return route.fulfill({ status: 500, json: problem(500, 'Internal Server Error') });
      }
      for (const { method: m, regex, names, handler } of compiled) {
        if (m !== method) continue;
        const match = regex.exec(path);
        if (!match) continue;
        const params = Object.fromEntries(names.map((name, i) => [name, match[i + 1] ?? '']));
        const reply = handler({
          url,
          params,
          mode: options.mode,
          persona: options.persona ?? 'member',
        });
        // A live stream stays open: the request is never answered.
        if (reply === PENDING) return;
        if (reply === undefined)
          return route.fulfill({ status: 404, json: problem(404, 'Not Found') });
        return route.fulfill({ json: reply });
      }
      log.unhandled.push(`${method} ${path}`);
      return route.fulfill({ status: 404, json: problem(404, 'Not Found') });
    },
  );

  // onto-intelli's UI, embedded by the map: a stand-in page that announces itself and shows
  // that the session reached it (the real UI is onto-intelli's to test).
  await page.route(/^http:\/\/onto\.test\//, (route) =>
    route.fulfill({ contentType: 'text/html', body: ONTO_UI }),
  );

  // The canvas's collaboration socket: opened, answered with pongs, otherwise silent.
  await page.routeWebSocket(/\/api\/v1\/graphs\/[^/]+\/ws/, (ws) => {
    ws.onMessage((message) => {
      if (typeof message === 'string' && message.includes('"ping"')) ws.send('{"type":"pong"}');
    });
  });
  return log;
}
