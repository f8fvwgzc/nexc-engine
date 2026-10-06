import { mapKindSchema, type MapItem, type MapKind } from '@/schemas/insight';

/** How each kind on the map is called, one and many. */
const NAMES: Record<string, [string, string]> = {
  member: ['member', 'members'],
  team: ['team', 'teams'],
  project: ['project', 'projects'],
  issue: ['issue', 'issues'],
  graph: ['graph', 'graphs'],
  document: ['document', 'documents'],
  agent: ['agent', 'agents'],
  label: ['label', 'labels'],
  cycle: ['cycle', 'cycles'],
  topic: ['topic', 'topics'],
  run: ['run', 'runs'],
  node: ['node', 'nodes'],
  memory: ['memory', 'memories'],
  passage: ['passage', 'passages'],
};

/** What one thing of a kind is called: "member", "issue". */
export function nameOf(kind: string): string {
  return NAMES[kind]?.[0] ?? kind;
}

/** "1 team", "14 issues"; a kind the app does not know yet is named as the server names it. */
export function countOf(kind: string, count: number): string {
  const [one, many] = NAMES[kind] ?? [kind, `${kind}s`];
  return `${new Intl.NumberFormat('en').format(count)} ${count === 1 ? one : many}`;
}

/** The kinds whose ties can be followed, as the picker shows them. */
export const FOLLOWABLE: { kind: MapKind; label: string }[] = mapKindSchema.options.map((kind) => {
  const many = NAMES[kind]?.[1] ?? kind;
  return { kind, label: many.charAt(0).toUpperCase() + many.slice(1) };
});

/** The kind of an item when its ties can be followed. */
export function followable(kind: string): MapKind | null {
  const parsed = mapKindSchema.safeParse(kind);
  return parsed.success ? parsed.data : null;
}

/** Where the thing itself lives in the app, when it has a page. */
export function pageOf(item: MapItem): string | null {
  switch (item.kind) {
    case 'issue':
      return `/app/issues?issue=${item.id}`;
    case 'graph':
      return `/app/graphs/${item.id}`;
    case 'project':
      return `/app/projects/${item.id}`;
    case 'team':
      return `/app/issues?team=${item.id}`;
    case 'member':
      return '/app/settings/members';
    case 'document':
      return '/app/settings/knowledge';
    case 'agent':
      return '/app/agents';
    default:
      return null;
  }
}
