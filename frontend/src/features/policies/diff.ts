/** A line of a document, and whether it is new, gone or the same as in the version before. */
export interface DiffLine {
  text: string;
  kind: 'same' | 'added' | 'removed';
}

/**
 * The lines of `after` against `before`, by content: a line only in `after` is added, one only
 * in `before` is removed. Documents are pretty-printed JSON, so a changed value is one removed
 * line next to one added line.
 */
export function diffLines(before: string | null, after: string): DiffLine[] {
  const next = after.split('\n');
  if (before === null) return next.map((text) => ({ text, kind: 'same' as const }));
  const previous = before.split('\n');
  const inBefore = new Map<string, number>();
  for (const line of previous) inBefore.set(line, (inBefore.get(line) ?? 0) + 1);
  const inAfter = new Map<string, number>();
  for (const line of next) inAfter.set(line, (inAfter.get(line) ?? 0) + 1);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < previous.length || j < next.length) {
    const p = previous[i] ?? '';
    const n = next[j] ?? '';
    if (i < previous.length && j < next.length && p === n) {
      out.push({ text: n, kind: 'same' });
      i += 1;
      j += 1;
    } else if (i < previous.length && (inAfter.get(p) ?? 0) === 0) {
      out.push({ text: p, kind: 'removed' });
      i += 1;
    } else if (j < next.length && (inBefore.get(n) ?? 0) === 0) {
      out.push({ text: n, kind: 'added' });
      j += 1;
    } else if (j < next.length) {
      // Moved lines read as a change: the old place is gone, the new one is new.
      if (i < previous.length) {
        out.push({ text: p, kind: 'removed' });
        i += 1;
      }
      out.push({ text: n, kind: 'added' });
      j += 1;
    } else {
      out.push({ text: p, kind: 'removed' });
      i += 1;
    }
  }
  return out;
}
