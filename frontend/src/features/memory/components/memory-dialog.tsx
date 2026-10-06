import { useQuery } from '@tanstack/react-query';
import { Trash2Icon } from 'lucide-react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { formatDateTime } from '@/lib/format';
import type { Memory } from '@/schemas/memory';

import { memoryQuery } from '../api';

/**
 * One memory in full. The list only carries a preview, so opening the dialog reads the memory
 * itself; the preview's facts show at once and the text follows.
 */
export function MemoryDialog({
  preview,
  graphName,
  onDelete,
  onClose,
}: {
  preview: Memory;
  graphName: string | undefined;
  onDelete: () => void;
  onClose: () => void;
}) {
  const { data: memory, error, isPending } = useQuery(memoryQuery(preview.id));
  const shown = memory ?? preview;
  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-base">
            <Badge variant="secondary" className="capitalize">
              {shown.kind}
            </Badge>
            Memory
          </DialogTitle>
          <DialogDescription>
            {shown.scope === 'user' ? 'Your own note' : `Learned in ${graphName ?? 'a graph'}`}
          </DialogDescription>
        </DialogHeader>
        {error ? (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(error)}
          </p>
        ) : isPending ? (
          <div className="space-y-2" aria-busy>
            <Skeleton className="h-4 w-full" />
            <Skeleton className="h-4 w-11/12" />
            <Skeleton className="h-4 w-2/3" />
          </div>
        ) : (
          <p className="max-h-[50vh] overflow-y-auto text-sm leading-relaxed break-words whitespace-pre-wrap">
            {shown.content}
          </p>
        )}
        <dl className="grid grid-cols-2 gap-x-4 gap-y-1 border-t pt-3 text-xs text-muted-foreground sm:grid-cols-4">
          <div>
            <dt>Importance</dt>
            <dd className="text-foreground tabular-nums">{shown.importance.toFixed(2)}</dd>
          </div>
          <div>
            <dt>Recalled</dt>
            <dd className="text-foreground tabular-nums">{shown.access_count}×</dd>
          </div>
          <div>
            <dt>Learned</dt>
            <dd className="text-foreground">{formatDateTime(shown.created_at)}</dd>
          </div>
          <div>
            <dt>Updated</dt>
            <dd className="text-foreground">{formatDateTime(shown.updated_at)}</dd>
          </div>
        </dl>
        <DialogFooter className="sm:justify-between">
          <Button variant="ghost" className="text-destructive" onClick={onDelete}>
            <Trash2Icon />
            Delete
          </Button>
          <Button variant="outline" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
