/**
 * A dynamic import that tries once more before it gives up, for a chunk that failed over a
 * connection that dropped for a moment. A browser that remembers the failed fetch fails the
 * second try at once, and a file a deploy removed fails both, so callers still need a fallback.
 */
export function retryImport<T>(load: () => Promise<T>): () => Promise<T> {
  return () => load().catch(() => load());
}
