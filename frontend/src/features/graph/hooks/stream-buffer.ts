import type { LogLine } from '@/stores/graph-store';

interface Pending {
  outputs: Record<string, string>;
  logs: Record<string, LogLine[]>;
}

export interface StreamBuffer {
  output: (runId: string, nodeId: string, delta: string) => void;
  log: (runId: string, nodeId: string, line: LogLine) => void;
  flush: () => void;
  cancel: () => void;
}

/**
 * Coalesces high-frequency `node.output` / `node.log` events into one store update per
 * `intervalMs`, so token streaming doesn't re-render React on every delta.
 */
export function createStreamBuffer(
  apply: (runId: string, pending: Pending) => void,
  intervalMs = 60,
): StreamBuffer {
  const pending = new Map<string, Pending>();
  let timer: ReturnType<typeof setTimeout> | null = null;

  const entry = (runId: string) => {
    let p = pending.get(runId);
    if (!p) {
      p = { outputs: {}, logs: {} };
      pending.set(runId, p);
    }
    return p;
  };

  const flush = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    for (const [runId, p] of pending) apply(runId, p);
    pending.clear();
  };

  const schedule = () => {
    timer ??= setTimeout(flush, intervalMs);
  };

  return {
    output(runId, nodeId, delta) {
      const p = entry(runId);
      p.outputs[nodeId] = (p.outputs[nodeId] ?? '') + delta;
      schedule();
    },
    log(runId, nodeId, line) {
      const p = entry(runId);
      (p.logs[nodeId] ??= []).push(line);
      schedule();
    },
    flush,
    cancel() {
      if (timer) clearTimeout(timer);
      timer = null;
      pending.clear();
    },
  };
}
