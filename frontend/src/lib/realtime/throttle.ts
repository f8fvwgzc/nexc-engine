/** Trailing-edge throttle keyed by an id: at most one call per key every `waitMs`, last args win. */
export function keyedThrottle<A extends unknown[]>(
  waitMs: number,
  fn: (key: string, ...args: A) => void,
): { call: (key: string, ...args: A) => void; flush: () => void; cancel: () => void } {
  const pending = new Map<string, A>();
  const lastRun = new Map<string, number>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  const run = (key: string) => {
    const args = pending.get(key);
    timers.delete(key);
    if (!args) return;
    pending.delete(key);
    lastRun.set(key, Date.now());
    fn(key, ...args);
  };

  return {
    call(key, ...args) {
      pending.set(key, args);
      if (timers.has(key)) return;
      const elapsed = Date.now() - (lastRun.get(key) ?? 0);
      if (elapsed >= waitMs) run(key);
      else
        timers.set(
          key,
          setTimeout(() => run(key), waitMs - elapsed),
        );
    },
    flush() {
      for (const [key, timer] of timers) {
        clearTimeout(timer);
        run(key);
      }
    },
    cancel() {
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      pending.clear();
    },
  };
}
