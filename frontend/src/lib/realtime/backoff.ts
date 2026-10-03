export interface BackoffOptions {
  baseMs?: number;
  maxMs?: number;
  /** 0..1 — fraction of the delay that is randomised to avoid reconnect stampedes. */
  jitter?: number;
}

/** Exponential backoff delay for the given (0-based) attempt, with jitter. */
export function backoffDelay(
  attempt: number,
  { baseMs = 500, maxMs = 30_000, jitter = 0.3 }: BackoffOptions = {},
  random: () => number = Math.random,
): number {
  const exp = Math.min(maxMs, baseMs * 2 ** Math.max(0, attempt));
  const spread = exp * jitter;
  return Math.round(exp - spread + random() * spread * 2);
}
