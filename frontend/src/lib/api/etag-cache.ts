import { env } from '@/lib/env';
import { CACHE_SCHEMA, MAX_ISSUE_LISTS, MAX_LIST_LENGTH } from '@/lib/query-persist';
import { useAuthStore } from '@/stores/auth-store';

/**
 * Conditional reads (CONTRACT §3). Every JSON read the server lets a browser keep carries a strong
 * `ETag` and `Cache-Control: private, no-cache`. This keeps the tag and the body of each GET by
 * URL, so the next request for the same URL sends `If-None-Match` and, when the server answers
 * `304 Not Modified` with no body, the body kept here is used again. Answers the server marks
 * `no-store` (sign-in and the account, credentials, tokens, downloads) carry no tag, so none of
 * them is kept here.
 *
 * Entries live for the page. The lists query-persist keeps between visits (the same data, in the
 * same browser storage) are kept here too with their tags, per account and `CACHE_SCHEMA` (so
 * across deploys that change no shape), and the first refetch of a visit is a 304 as well;
 * nothing else reaches storage. The same limits apply: the issue lists read last, and no list
 * too long, or this copy would fill the storage query-persist needs.
 */

interface Entry {
  etag: string;
  body: unknown;
}

interface Snapshot {
  schema: number;
  userId: string;
  savedAt: number;
  entries: [string, Entry][];
}

const STORAGE_KEY = 'nexc.etags.v1';
const MAX_ENTRIES = 500;
const MAX_PERSISTED = 100;
const MAX_AGE_MS = 24 * 60 * 60 * 1000;
const SAVE_DELAY_MS = 1_000;

/** The lists worth a tag between visits: what query-persist keeps, as paths. */
const PERSISTED_PATHS =
  /^\/(workspaces|graphs|agents|templates|agent-templates)(\?|$)|^\/workspaces\/[^/]+\/(teams|issues|labels|projects)(\?|$)/;
/** Searches and sub-issue lists, which query-persist leaves out too: one per string, per issue. */
const UNKEPT_PARAMS = /[?&](q|parent_id)=/;
/** A workspace's issue list, whatever it is filtered by. */
const ISSUE_LIST = /^\/workspaces\/[^/]+\/issues(\?|$)/;

const entries = new Map<string, Entry>();
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let persisting = false;

function pathOf(url: string): string {
  return url.startsWith(env.apiBaseUrl) ? url.slice(env.apiBaseUrl.length) : url;
}

export function persistable(url: string): boolean {
  const path = pathOf(url);
  return PERSISTED_PATHS.test(path) && !UNKEPT_PARAMS.test(path);
}

/** The `If-None-Match` header for `url`, when a body is kept for it. */
export function conditionalHeaders(url: string): Record<string, string> {
  const entry = entries.get(url);
  return entry ? { 'If-None-Match': entry.etag } : {};
}

/** The body kept for `url`, or `undefined`. */
export function cachedBody(url: string): unknown {
  return entries.get(url)?.body;
}

/** Keeps the answer of a read; the oldest entry goes when the cache is full. */
export function remember(url: string, etag: string, body: unknown): void {
  entries.delete(url);
  entries.set(url, { etag, body });
  while (entries.size > MAX_ENTRIES) {
    const oldest = entries.keys().next().value;
    if (oldest === undefined) break;
    entries.delete(oldest);
  }
  if (persisting && persistable(url)) scheduleSave();
}

export function forget(url: string): void {
  entries.delete(url);
}

/** Drops everything kept, in memory and in storage (a sign-out). */
export function forgetAll(): void {
  entries.clear();
  clearTimeout(saveTimer);
  saveTimer = undefined;
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Storage is unavailable; there is nothing to forget.
  }
}

function scheduleSave(): void {
  saveTimer ??= setTimeout(() => {
    saveTimer = undefined;
    save();
  }, SAVE_DELAY_MS);
}

/**
 * What goes to storage, oldest first: the persisted lists but those too long to keep, and of the
 * issue lists only the ones remembered last (the map is in the order they were).
 */
function persistedEntries(): [string, Entry][] {
  let issueLists = 0;
  return [...entries.entries()]
    .filter(([url, { body }]) => {
      if (!persistable(url)) return false;
      return !(Array.isArray(body) && body.length > MAX_LIST_LENGTH);
    })
    .reverse()
    .filter(([url]) => !ISSUE_LIST.test(pathOf(url)) || ++issueLists <= MAX_ISSUE_LISTS)
    .slice(0, MAX_PERSISTED)
    .reverse();
}

function save(): void {
  const userId = useAuthStore.getState().user?.id;
  if (!userId) return;
  const kept = persistedEntries();
  try {
    const snapshot: Snapshot = {
      schema: CACHE_SCHEMA,
      userId,
      savedAt: Date.now(),
      entries: kept,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(snapshot));
  } catch {
    // Over quota or storage disabled: the app works without the copy.
  }
}

function read(): Snapshot | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Snapshot) : null;
  } catch {
    return null;
  }
}

function restore(userId: string): void {
  const snapshot = read();
  if (
    !snapshot ||
    snapshot.schema !== CACHE_SCHEMA ||
    snapshot.userId !== userId ||
    Date.now() - snapshot.savedAt > MAX_AGE_MS ||
    !Array.isArray(snapshot.entries)
  ) {
    forgetAll();
    return;
  }
  for (const [url, entry] of snapshot.entries) {
    if (typeof url === 'string' && typeof entry?.etag === 'string' && persistable(url)) {
      entries.set(url, entry);
    }
  }
}

/**
 * Restores the tags kept for the signed-in account and keeps saving the persisted lists' tags;
 * forgets everything on sign-out. Returns a function that stops it.
 */
export function startEtagPersistence(): () => void {
  persisting = true;
  let restoredFor: string | null = null;
  const onAuth = (status: string, userId: string | undefined) => {
    if (status === 'authenticated' && userId && restoredFor !== userId) {
      restoredFor = userId;
      restore(userId);
    }
    if (status === 'anonymous') {
      restoredFor = null;
      forgetAll();
    }
  };
  const initial = useAuthStore.getState();
  onAuth(initial.status, initial.user?.id);
  const stop = useAuthStore.subscribe((state) => onAuth(state.status, state.user?.id));
  const flush = () => {
    if (saveTimer === undefined) return;
    clearTimeout(saveTimer);
    saveTimer = undefined;
    save();
  };
  window.addEventListener('pagehide', flush);
  return () => {
    persisting = false;
    clearTimeout(saveTimer);
    saveTimer = undefined;
    window.removeEventListener('pagehide', flush);
    stop();
  };
}
