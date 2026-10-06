import { Badge } from '@/components/ui/badge';
import { formatRelative } from '@/lib/format';
import type { Memory } from '@/schemas/memory';

interface MemoryListProps {
  /** Previews: `content` may be cut short. */
  memories: Memory[];
  graphNames: Map<string, string>;
  onOpen: (memory: Memory) => void;
}

/** Memories as rows: kind, a one-line preview, where it was learned, and when. */
export function MemoryList({ memories, graphNames, onOpen }: MemoryListProps) {
  return (
    <ul className="divide-y overflow-hidden rounded-lg border">
      {memories.map((memory) => (
        <li key={memory.id}>
          <button
            type="button"
            onClick={() => onOpen(memory)}
            className="flex min-h-10 w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] transition-colors outline-none hover:bg-muted/60 focus-visible:bg-muted/60"
          >
            <Badge variant="secondary" className="w-24 shrink-0 justify-center capitalize">
              {memory.kind}
            </Badge>
            <span className="min-w-0 flex-1 truncate">{memory.content}</span>
            {memory.score !== null && (
              <span className="shrink-0 text-xs font-medium text-brand tabular-nums">
                {(memory.score * 100).toFixed(0)}%
              </span>
            )}
            {memory.graph_id && (
              <span className="hidden max-w-40 shrink-0 truncate text-xs text-muted-foreground md:block">
                {graphNames.get(memory.graph_id) ?? 'graph'}
              </span>
            )}
            <span className="w-24 shrink-0 text-right text-xs text-muted-foreground tabular-nums">
              {formatRelative(memory.updated_at)}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
