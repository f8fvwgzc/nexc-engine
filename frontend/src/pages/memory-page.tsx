import { useQuery } from '@tanstack/react-query';
import { BrainIcon, SearchIcon } from 'lucide-react';
import { useState } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageHeader } from '@/components/custom-ui/page-header';
import { TableSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group';
import { Spinner } from '@/components/ui/spinner';
import { graphsQuery } from '@/features/graphs/api';
import { memoriesQuery } from '@/features/memory/api';
import { MemoryList } from '@/features/memory/components/memory-list';
import { useDeleteMemory } from '@/features/memory/hooks/use-delete-memory';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import type { Memory } from '@/schemas/memory';

const ALL_GRAPHS = '__all__';
const LIMIT = 50;

export default function MemoryPage() {
  const [q, setQ] = useState('');
  const [graphId, setGraphId] = useState(ALL_GRAPHS);
  const [deleting, setDeleting] = useState<Memory | null>(null);
  const debouncedQ = useDebouncedValue(q.trim(), 300);
  const { data: graphs = [] } = useQuery(graphsQuery());
  const {
    data: memories,
    isPending,
    isFetching,
  } = useQuery(
    memoriesQuery({
      q: debouncedQ || undefined,
      graph_id: graphId === ALL_GRAPHS ? undefined : graphId,
      limit: LIMIT,
    }),
  );
  const deleteMemory = useDeleteMemory();
  const graphNames = new Map(graphs.map((g) => [g.id, g.name]));

  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <Seo title="Memory" noIndex />
      <PageHeader
        title="Memory"
        description="Facts, experiences and preferences agents have learned. They are retrieved into node prompts at run time."
      />
      <div className="flex flex-col gap-2 sm:flex-row">
        <InputGroup className="flex-1">
          <InputGroupAddon>
            {isFetching && !isPending ? <Spinner /> : <SearchIcon />}
          </InputGroupAddon>
          <InputGroupInput
            type="search"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Search memories semantically…"
            aria-label="Search memories"
          />
        </InputGroup>
        <OptionSelect
          className="sm:w-56"
          aria-label="Filter by graph"
          value={graphId}
          onValueChange={setGraphId}
          options={[
            { value: ALL_GRAPHS, label: 'All graphs' },
            ...graphs.map((g) => ({ value: g.id, label: g.name })),
          ]}
        />
      </div>
      {isPending ? (
        <TableSkeleton rows={4} />
      ) : memories && memories.length > 0 ? (
        <MemoryList memories={memories} graphNames={graphNames} onDelete={setDeleting} />
      ) : (
        <EmptyState
          icon={BrainIcon}
          title={debouncedQ ? 'No matching memories' : 'No memories yet'}
          description={
            debouncedQ
              ? 'Try different words — search is semantic, not exact.'
              : 'Memories are written by agents as they run your graphs.'
          }
        />
      )}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title="Delete this memory?"
        description="Agents will no longer recall it in future runs."
        confirmLabel="Delete"
        destructive
        onConfirm={() => deleting && deleteMemory.mutate(deleting.id)}
      />
    </div>
  );
}
