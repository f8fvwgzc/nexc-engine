import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BrainIcon, ChevronLeftIcon, ChevronRightIcon, SearchIcon } from 'lucide-react';
import { useState } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageHeader } from '@/components/custom-ui/page-header';
import { TableSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group';
import { Spinner } from '@/components/ui/spinner';
import { graphsQuery } from '@/features/graphs/api';
import { memoriesQuery, memoryTopicsQuery, rebuildMemoryTopics } from '@/features/memory/api';
import { MemoryDialog } from '@/features/memory/components/memory-dialog';
import { MemoryList } from '@/features/memory/components/memory-list';
import { useDeleteMemory } from '@/features/memory/hooks/use-delete-memory';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import type { Memory } from '@/schemas/memory';
import { useCurrentWorkspace, useWorkspaceId } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';

const ALL_GRAPHS = '__all__';
/** Memories per page. One more is asked for, to know whether a next page exists. */
const PAGE_SIZE = 10;
/** Characters of each memory the list carries; the rest is read when one is opened. */
const PREVIEW_CHARS = 160;

export default function MemoryPage() {
  const [q, setQ] = useState('');
  const [graphId, setGraphId] = useState(ALL_GRAPHS);
  const [deleting, setDeleting] = useState<Memory | null>(null);
  const [opened, setOpened] = useState<Memory | null>(null);
  const [page, setPage] = useState(0);
  const [topicId, setTopicId] = useState<string | null>(null);
  const debouncedQ = useDebouncedValue(q.trim(), 300);
  const workspaceId = useWorkspaceId();
  const queryClient = useQueryClient();
  const { current } = useCurrentWorkspace();
  const admin = current?.role === 'owner' || current?.role === 'admin';
  const { data: topics = [] } = useQuery(memoryTopicsQuery(workspaceId ?? ''));
  // A topic that a rebuild replaced stops filtering.
  const topic = topics.find((t) => t.id === topicId);
  const rebuild = useMutation({
    mutationFn: () => rebuildMemoryTopics(workspaceId ?? ''),
    meta: { successMessage: 'Finding topics again; this takes a moment' },
    onSuccess: () => {
      // The work runs in the background: look again shortly.
      setTimeout(() => void queryClient.invalidateQueries({ queryKey: qk.memories.all }), 3_000);
    },
  });
  const { data: graphs = [] } = useQuery(graphsQuery(workspaceId));
  // A page belongs to one search and one graph filter; changing either starts over.
  const filterKey = `${debouncedQ}|${graphId}|${topic?.id ?? ''}`;
  const [pagedFor, setPagedFor] = useState(filterKey);
  if (pagedFor !== filterKey) {
    setPagedFor(filterKey);
    setPage(0);
  }
  const {
    data: fetched,
    isPending,
    isFetching,
  } = useQuery(
    memoriesQuery({
      workspace_id: workspaceId,
      q: debouncedQ || undefined,
      graph_id: graphId === ALL_GRAPHS ? undefined : graphId,
      topic_id: topic?.id,
      limit: PAGE_SIZE + 1,
      offset: page * PAGE_SIZE,
      preview: PREVIEW_CHARS,
    }),
  );
  const memories = fetched?.slice(0, PAGE_SIZE);
  const hasNext = (fetched?.length ?? 0) > PAGE_SIZE;
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
      {(topics.length > 0 || admin) && (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="mr-1 text-xs text-muted-foreground">Topics</span>
          {topics.length === 0 && (
            <span className="text-xs text-muted-foreground">
              appear once the workspace has about twenty shared memories
            </span>
          )}
          {topics.map((t) => (
            <button
              key={t.id}
              type="button"
              aria-pressed={t.id === topic?.id}
              onClick={() => setTopicId(t.id === topic?.id ? null : t.id)}
              className="inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 text-xs transition-colors outline-none hover:bg-muted/60 focus-visible:border-ring aria-pressed:border-ring aria-pressed:bg-muted"
            >
              {t.label}
              <span className="text-muted-foreground tabular-nums">{t.memory_count}</span>
            </button>
          ))}
          {admin && (
            <Button
              variant="ghost"
              size="sm"
              className="ml-auto h-6 px-2 text-xs"
              disabled={rebuild.isPending}
              onClick={() => rebuild.mutate()}
            >
              Find topics again
            </Button>
          )}
        </div>
      )}
      {isPending ? (
        <TableSkeleton rows={4} />
      ) : memories && (memories.length > 0 || page > 0) ? (
        <div className="space-y-3">
          <MemoryList memories={memories} graphNames={graphNames} onOpen={setOpened} />
          {(page > 0 || hasNext) && (
            <nav
              aria-label="Pages"
              className="flex items-center justify-end gap-2 text-[13px] text-muted-foreground"
            >
              <span className="mr-1 tabular-nums">Page {page + 1}</span>
              <Button
                variant="outline"
                size="sm"
                disabled={page === 0 || isFetching}
                onClick={() => setPage((p) => Math.max(0, p - 1))}
              >
                <ChevronLeftIcon />
                Previous
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={!hasNext || isFetching}
                onClick={() => setPage((p) => p + 1)}
              >
                Next
                <ChevronRightIcon />
              </Button>
            </nav>
          )}
        </div>
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
      {opened && (
        <MemoryDialog
          key={opened.id}
          preview={opened}
          graphName={opened.graph_id ? graphNames.get(opened.graph_id) : undefined}
          onDelete={() => setDeleting(opened)}
          onClose={() => setOpened(null)}
        />
      )}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title="Delete this memory?"
        description="Agents will no longer recall it in future runs."
        confirmLabel="Delete"
        destructive
        onConfirm={() => {
          if (!deleting) return;
          deleteMemory.mutate(deleting.id);
          setOpened(null);
        }}
      />
    </div>
  );
}
