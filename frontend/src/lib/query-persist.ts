import {
  dehydrate,
  hydrate,
  type DehydratedState,
  type Query,
  type QueryClient,
  type QueryKey,
} from '@tanstack/react-query';

import { useAuthStore } from '@/stores/auth-store';

/**
 * The shape of what this browser keeps between visits (here and in etag-cache). A copy written
 * under another number is dropped instead of restored, so a deploy keeps the copy (and its first
 * paint and 304s) unless the shapes changed. Bump it whenever a kept list changes shape.
 *
 * 2: issue lists are slim (no description) and carry their graph's last run status.
 */
export const CACHE_SCHEMA = 2;

const STORAGE_KEY = 'nexc.query-cache.v1';
const MAX_AGE_MS = 24 * 60 * 60 * 1000;
const SAVE_DELAY_MS = 1_000;
/**
 * The issue lists kept: the ones read last. A filter tried once does not stay for a day.
 * etag-cache keeps the same ones (both copies share the origin's storage).
 */
export const MAX_ISSUE_LISTS = 8;
/** A list longer than this is not kept: it would crowd out the rest of the copy. */
export const MAX_LIST_LENGTH = 500;

/**
 * Lists worth painting instantly on the next visit. Settings (credential hints), memories, run
 * outputs, whole graphs and the API tokens of agents stay out: they are sensitive, large, or
 * both.
 */
const PERSISTED_ROOTS = new Set(['workspaces', 'graphs', 'issues', 'agents', 'templates']);

/**
 * Of the issue reads, the lists, workflows, projects, cycles, labels and the inbox. Not an
 * issue's timeline, nor a search (a list per string typed), nor sub-issues (a list per issue).
 */
function keptIssueRead(queryKey: QueryKey): boolean {
  if (queryKey[1] === 'events') return false;
  if (queryKey[2] !== 'list') return true;
  const filter = queryKey[3];
  if (typeof filter !== 'object' || filter === null) return true;
  const { q, parent_id } = filter as { q?: unknown; parent_id?: unknown };
  return !q && !parent_id;
}

function persistable(queryKey: QueryKey): boolean {
  const [root] = queryKey;
  return (
    typeof root === 'string' &&
    PERSISTED_ROOTS.has(root) &&
    !queryKey.includes('detail') &&
    !queryKey.includes('tokens') &&
    (root !== 'issues' || keptIssueRead(queryKey))
  );
}

const isIssueList = (queryKey: QueryKey) => queryKey[0] === 'issues' && queryKey[2] === 'list';

interface Snapshot {
  /** The {@link CACHE_SCHEMA} the copy was written under. */
  schema: number;
  userId: string;
  savedAt: number;
  state: DehydratedState;
}

function read(): Snapshot | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Snapshot) : null;
  } catch {
    return null;
  }
}

function forget(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Storage is unavailable; there is nothing to forget.
  }
}

/**
 * Keeps a copy of the list queries in this browser so pages paint from it on the next visit and
 * refresh in the background (restored lists are marked stale). The copy belongs to one account
 * and one {@link CACHE_SCHEMA}: it is restored only for the user and the schema that wrote it,
 * so it outlives a deploy that changes no shape, and it is removed when they sign out.
 */
export function startQueryPersistence(client: QueryClient): () => void {
  let restoredFor: string | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const restore = (userId: string) => {
    if (restoredFor === userId) return;
    restoredFor = userId;
    const snapshot = read();
    if (
      !snapshot ||
      snapshot.schema !== CACHE_SCHEMA ||
      snapshot.userId !== userId ||
      Date.now() - snapshot.savedAt > MAX_AGE_MS
    ) {
      forget();
      return;
    }
    try {
      hydrate(client, snapshot.state);
      // The copy may predate the last change made before the page was left, however recent its
      // timestamp, so it is only ever a first paint: every restored list is fetched again.
      void client.invalidateQueries({ predicate: (query) => persistable(query.queryKey) });
    } catch {
      forget();
    }
  };

  const save = () => {
    const userId = useAuthStore.getState().user?.id;
    if (!userId) return;
    const kept = (query: Query) =>
      query.state.status === 'success' &&
      persistable(query.queryKey) &&
      !(Array.isArray(query.state.data) && query.state.data.length > MAX_LIST_LENGTH);
    const issueLists = new Set(
      client
        .getQueryCache()
        .findAll({ predicate: (query) => isIssueList(query.queryKey) && kept(query) })
        .sort((a, b) => b.state.dataUpdatedAt - a.state.dataUpdatedAt)
        .slice(0, MAX_ISSUE_LISTS)
        .map((query) => query.queryHash),
    );
    const state = dehydrate(client, {
      shouldDehydrateQuery: (query) =>
        kept(query) && (!isIssueList(query.queryKey) || issueLists.has(query.queryHash)),
    });
    try {
      const snapshot: Snapshot = { schema: CACHE_SCHEMA, userId, savedAt: Date.now(), state };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(snapshot));
    } catch {
      // Over quota or storage disabled: the app works without the copy.
      forget();
    }
  };

  const onAuth = (status: string, userId: string | undefined) => {
    if (status === 'authenticated' && userId) restore(userId);
    if (status === 'anonymous') {
      restoredFor = null;
      clearTimeout(timer);
      timer = undefined;
      forget();
    }
  };
  const initial = useAuthStore.getState();
  onAuth(initial.status, initial.user?.id);
  const stopAuth = useAuthStore.subscribe((state) => onAuth(state.status, state.user?.id));
  const stopCache = client.getQueryCache().subscribe((event) => {
    const queryKey: QueryKey = event.query.queryKey as QueryKey;
    if (event.type !== 'updated' || !persistable(queryKey)) return;
    // One save per burst of updates; a steady stream must not keep postponing it.
    timer ??= setTimeout(() => {
      timer = undefined;
      save();
    }, SAVE_DELAY_MS);
  });
  // Leaving the page inside the delay would lose the last change.
  const flush = () => {
    if (timer === undefined) return;
    clearTimeout(timer);
    timer = undefined;
    save();
  };
  window.addEventListener('pagehide', flush);
  return () => {
    clearTimeout(timer);
    window.removeEventListener('pagehide', flush);
    stopAuth();
    stopCache();
  };
}
