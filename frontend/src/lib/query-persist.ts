import {
  dehydrate,
  hydrate,
  type DehydratedState,
  type QueryClient,
  type QueryKey,
} from '@tanstack/react-query';

import { useAuthStore } from '@/stores/auth-store';

const STORAGE_KEY = 'nexc.query-cache.v1';
const MAX_AGE_MS = 24 * 60 * 60 * 1000;
const SAVE_DELAY_MS = 1_000;

/**
 * Lists worth painting instantly on the next visit. Settings (credential hints), memories, run
 * outputs and whole graphs stay out: they are sensitive, large, or both.
 */
const PERSISTED_ROOTS = new Set(['workspaces', 'graphs', 'issues', 'agents', 'templates']);

function persistable(queryKey: QueryKey): boolean {
  return (
    typeof queryKey[0] === 'string' &&
    PERSISTED_ROOTS.has(queryKey[0]) &&
    !queryKey.includes('detail')
  );
}

interface Snapshot {
  /** The build that wrote the copy: another build may expect lists of another shape. */
  build: string;
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
 * refresh in the background (restored lists are marked stale). The copy belongs to
 * one account and one build of the app: it is restored only for the user and the build that wrote
 * it, and removed when they sign out.
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
      snapshot.build !== __NEXC_BUILD__ ||
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
    const state = dehydrate(client, {
      shouldDehydrateQuery: (query) =>
        query.state.status === 'success' && persistable(query.queryKey),
    });
    try {
      const snapshot: Snapshot = { build: __NEXC_BUILD__, userId, savedAt: Date.now(), state };
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
