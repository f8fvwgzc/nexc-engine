export { cn } from 'cn';

/** Returns a same-origin in-app path for a `?next=` value, rejecting open redirects. */
export function safeNextPath(next: string | null | undefined, fallback = '/app'): string {
  if (!next || !next.startsWith('/') || next.startsWith('//') || next.startsWith('/\\')) {
    return fallback;
  }
  return next;
}
