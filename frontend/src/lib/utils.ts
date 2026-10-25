export { cn } from 'cn';

/** Returns a same-origin in-app path for a `?next=` value, rejecting open redirects. */
export function safeNextPath(next: string | null | undefined, fallback = '/app'): string {
  if (!next || !next.startsWith('/') || next.startsWith('//') || next.startsWith('/\\')) {
    return fallback;
  }
  return next;
}

/** Whether every word of a search appears in one of the fields, ignoring case (filter boxes). */
export function matchesWords(fields: readonly string[], search: string): boolean {
  const words = search.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const haystack = fields.join(' ').toLowerCase();
  return words.every((word) => haystack.includes(word));
}
