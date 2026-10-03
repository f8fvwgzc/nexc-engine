import { Trash2Icon } from 'lucide-react';

import { Stagger } from '@/components/custom-ui/motion';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { formatRelative } from '@/lib/format';
import type { Memory } from '@/schemas/memory';

interface MemoryListProps {
  memories: Memory[];
  graphNames: Map<string, string>;
  onDelete: (memory: Memory) => void;
}

export function MemoryList({ memories, graphNames, onDelete }: MemoryListProps) {
  return (
    <Stagger className="space-y-2" stepMs={30}>
      {memories.map((memory) => (
        <article
          key={memory.id}
          className="group flex gap-3 rounded-xl border bg-card/60 p-4 transition-colors hover:border-foreground/15"
        >
          <div className="min-w-0 flex-1 space-y-2">
            <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
              <Badge variant="secondary" className="capitalize">
                {memory.kind}
              </Badge>
              <span className="capitalize">{memory.scope}</span>
              {memory.graph_id && (
                <span className="truncate">· {graphNames.get(memory.graph_id) ?? 'graph'}</span>
              )}
              <span>· importance {memory.importance.toFixed(2)}</span>
              <span>· used {memory.access_count}×</span>
              {memory.score !== null && (
                <span className="font-medium text-brand">
                  · match {(memory.score * 100).toFixed(0)}%
                </span>
              )}
              <span className="ml-auto">{formatRelative(memory.updated_at)}</span>
            </div>
            <p className="text-sm leading-relaxed break-words whitespace-pre-wrap">
              {memory.content}
            </p>
          </div>
          <Button
            variant="ghost"
            size="icon-sm"
            className="shrink-0 text-muted-foreground opacity-60 group-hover:opacity-100 hover:text-destructive focus-visible:opacity-100"
            aria-label="Delete memory"
            onClick={() => onDelete(memory)}
          >
            <Trash2Icon />
          </Button>
        </article>
      ))}
    </Stagger>
  );
}
