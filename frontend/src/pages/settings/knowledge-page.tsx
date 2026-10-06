import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BookOpenIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  FileTextIcon,
  SearchIcon,
  Trash2Icon,
  UploadIcon,
} from 'lucide-react';
import { useRef, useState } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Switch } from '@/components/ui/switch';
import {
  deleteDocument,
  documentsQuery,
  knowledgeSearchQuery,
  knowledgeSettingsQuery,
  rebuildTopics,
  saveKnowledgeSettings,
  topicsQuery,
  uploadDocument,
} from '@/features/knowledge/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { errorMessage } from '@/lib/api/errors';
import { formatBytes, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import type {
  DocumentStatus,
  KnowledgeDocument,
  KnowledgeSettings,
  KnowledgeSettingsInput,
} from '@/schemas/knowledge';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';

const PAGE_SIZE = 10;
const MAX_BYTES = 50 * 1024 * 1024;

const STATUS_LABEL: Record<DocumentStatus, string> = {
  pending: 'Waiting',
  parsing: 'Reading',
  embedding: 'Indexing',
  ready: 'Ready',
  failed: 'Failed',
};

/** Drop files or pick them; they are sent one after another and each reports its own outcome. */
function Uploader({ workspace, onDone }: { workspace: Workspace; onDone: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  const [over, setOver] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [problems, setProblems] = useState<string[]>([]);
  const send = async (files: File[]) => {
    const failed: string[] = [];
    for (const file of files) {
      if (file.size > MAX_BYTES) {
        failed.push(`${file.name}: larger than 50 MB`);
        continue;
      }
      setBusy(file.name);
      try {
        await uploadDocument(workspace.id, file);
      } catch (error) {
        failed.push(`${file.name}: ${errorMessage(error)}`);
      }
    }
    setBusy(null);
    setProblems(failed);
    onDone();
  };
  return (
    <div className="space-y-2">
      <div
        data-over={over}
        onDragOver={(e) => {
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          void send([...e.dataTransfer.files]);
        }}
        className="flex flex-col items-center gap-2 rounded-lg border border-dashed px-4 py-6 text-center text-[13px] text-muted-foreground transition-colors data-[over=true]:border-ring data-[over=true]:bg-muted/50"
      >
        <UploadIcon className="size-5" aria-hidden />
        <p>
          {busy
            ? `Uploading ${busy}…`
            : 'Drop files here: PDF, Word, Excel, PowerPoint, HTML, CSV, Markdown or text, up to 50 MB each.'}
        </p>
        <Button
          variant="outline"
          size="sm"
          disabled={busy !== null}
          onClick={() => input.current?.click()}
        >
          Choose files
        </Button>
        <input
          ref={input}
          type="file"
          multiple
          className="sr-only"
          aria-label="Files to add to the knowledge base"
          onChange={(e) => {
            void send([...(e.target.files ?? [])]);
            e.target.value = '';
          }}
        />
      </div>
      {problems.length > 0 && (
        <ul role="alert" className="space-y-0.5 text-[13px] text-destructive">
          {problems.map((problem) => (
            <li key={problem}>{problem}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

function DocumentRow({
  document,
  canDelete,
  onDelete,
}: {
  document: KnowledgeDocument;
  canDelete: boolean;
  onDelete: () => void;
}) {
  const working = document.status !== 'ready' && document.status !== 'failed';
  return (
    <li className="flex min-h-10 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-[13px]">
      <FileTextIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
      <span className="min-w-0 flex-1 basis-48">
        <span className="block truncate font-medium">{document.name}</span>
        {document.status === 'failed' && (
          <span className="block text-xs text-destructive">{document.error}</span>
        )}
      </span>
      <span className="w-28 text-xs text-muted-foreground tabular-nums">
        {document.page_count !== null ? `${document.page_count} pages · ` : ''}
        {document.chunk_count} passages
      </span>
      <span className="w-16 text-right text-xs text-muted-foreground tabular-nums">
        {formatBytes(document.size_bytes)}
      </span>
      <Badge
        variant={
          document.status === 'failed'
            ? 'destructive'
            : document.status === 'ready'
              ? 'outline'
              : 'secondary'
        }
        className={`w-20 justify-center ${working ? 'motion-safe:animate-pulse' : ''}`}
      >
        {STATUS_LABEL[document.status]}
      </Badge>
      <span className="hidden w-24 text-right text-xs text-muted-foreground sm:block">
        {formatRelative(document.created_at)}
      </span>
      <Button
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground hover:text-destructive"
        aria-label={`Remove ${document.name}`}
        disabled={!canDelete}
        onClick={onDelete}
      >
        <Trash2Icon />
      </Button>
    </li>
  );
}

function Documents({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const [q, setQ] = useState('');
  const [page, setPage] = useState(0);
  const [removing, setRemoving] = useState<KnowledgeDocument | null>(null);
  const name = useDebouncedValue(q.trim(), 300);
  const [pagedFor, setPagedFor] = useState(name);
  if (pagedFor !== name) {
    setPagedFor(name);
    setPage(0);
  }
  // One more than a page is asked for, to know whether a next page exists.
  const { data: fetched, isPending } = useQuery(
    documentsQuery(workspace.id, {
      q: name || undefined,
      limit: PAGE_SIZE + 1,
      offset: page * PAGE_SIZE,
    }),
  );
  const documents = fetched?.slice(0, PAGE_SIZE) ?? [];
  const hasNext = (fetched?.length ?? 0) > PAGE_SIZE;
  const reload = () => queryClient.invalidateQueries({ queryKey: qk.knowledge.all });
  const remove = useMutation({
    mutationFn: (id: string) => deleteDocument(workspace.id, id),
    meta: { successMessage: 'Document removed' },
    onSuccess: reload,
  });
  const admin = isWorkspaceAdmin(workspace.role);

  return (
    <section aria-label="Documents" className="space-y-3">
      <Uploader workspace={workspace} onDone={() => void reload()} />
      <Input
        type="search"
        value={q}
        placeholder="Filter by file name…"
        aria-label="Filter documents by name"
        className="h-8 max-w-xs text-[13px]"
        onChange={(e) => setQ(e.target.value)}
      />
      {isPending ? (
        <div className="space-y-2" aria-busy>
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} className="h-10 w-full" />
          ))}
        </div>
      ) : documents.length === 0 && page === 0 ? (
        <EmptyState
          icon={BookOpenIcon}
          title={name ? 'No document with that name' : 'No documents yet'}
          description={
            name
              ? 'Try fewer letters.'
              : 'Add the files your workspace works from. Agents and the planner read the passages that bear on their task.'
          }
        />
      ) : (
        <ul className="divide-y overflow-hidden rounded-lg border">
          {documents.map((document) => (
            <DocumentRow
              key={document.id}
              document={document}
              canDelete={admin || document.uploaded_by === me}
              onDelete={() => setRemoving(document)}
            />
          ))}
        </ul>
      )}
      {(page > 0 || hasNext) && (
        <nav
          aria-label="Pages"
          className="flex items-center justify-end gap-2 text-[13px] text-muted-foreground"
        >
          <span className="mr-1 tabular-nums">Page {page + 1}</span>
          <Button
            variant="outline"
            size="sm"
            disabled={page === 0}
            onClick={() => setPage((p) => Math.max(0, p - 1))}
          >
            <ChevronLeftIcon />
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={!hasNext}
            onClick={() => setPage((p) => p + 1)}
          >
            Next
            <ChevronRightIcon />
          </Button>
        </nav>
      )}
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title={`Remove ${removing?.name ?? 'this document'}?`}
        description="Its passages are removed with it; agents no longer read from it."
        confirmLabel="Remove"
        destructive
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </section>
  );
}

/** Asks the knowledge base a question the way a node would, and shows what comes back. */
function TrySearch({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const [q, setQ] = useState('');
  const [topicId, setTopicId] = useState<string | null>(null);
  const query = useDebouncedValue(q.trim(), 400);
  const { data: topics = [] } = useQuery(topicsQuery(workspace.id));
  // A topic that a rebuild replaced stops filtering.
  const topic = topics.find((t) => t.id === topicId);
  const {
    data: passages,
    isFetching,
    error,
  } = useQuery(knowledgeSearchQuery(workspace.id, query, topic?.id));
  const rebuild = useMutation({
    mutationFn: () => rebuildTopics(workspace.id),
    meta: { successMessage: 'Finding topics again; this takes a moment' },
    onSuccess: () => {
      // The work runs in the background: look again shortly.
      setTimeout(
        () => void queryClient.invalidateQueries({ queryKey: qk.knowledge.topics(workspace.id) }),
        3_000,
      );
    },
  });
  const topicLabel = new Map(topics.map((t) => [t.id, t.label]));
  return (
    <section aria-label="Search" className="space-y-3">
      <div className="flex items-center gap-2">
        <h2 className="text-[13px] font-medium">Topics</h2>
        <span className="text-xs text-muted-foreground">
          found by grouping similar passages; pick one to search within it
        </span>
        {isWorkspaceAdmin(workspace.role) && (
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
      {topics.length === 0 ? (
        <p className="text-[13px] text-muted-foreground">
          Topics appear once the workspace has about twenty passages.
        </p>
      ) : (
        <ul aria-label="Topics" className="flex flex-wrap gap-1.5">
          {topics.map((t) => (
            <li key={t.id}>
              <button
                type="button"
                aria-pressed={t.id === topic?.id}
                onClick={() => setTopicId(t.id === topic?.id ? null : t.id)}
                className="inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 text-xs transition-colors outline-none hover:bg-muted/60 focus-visible:border-ring aria-pressed:border-ring aria-pressed:bg-muted"
              >
                {t.label}
                <span className="text-muted-foreground tabular-nums">{t.chunk_count}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <h2 className="pt-2 text-[13px] font-medium">Try a search</h2>
      <div className="relative max-w-xl">
        <SearchIcon className="absolute top-2 left-2.5 size-4 text-muted-foreground" aria-hidden />
        <Input
          type="search"
          value={q}
          placeholder="Ask what a node would ask, e.g. customs reference for invoices"
          aria-label="Search the knowledge base"
          className="h-8 pl-8 text-[13px]"
          onChange={(e) => setQ(e.target.value)}
        />
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(error)}
        </p>
      )}
      {query && !isFetching && passages?.length === 0 && (
        <p className="text-[13px] text-muted-foreground">
          No passage shares a telling word with that question.
        </p>
      )}
      {query && passages && passages.length > 0 && (
        <ol className="space-y-2">
          {passages.map((passage) => (
            <li key={passage.chunk_id} className="rounded-lg border px-3 py-2">
              <p className="flex flex-wrap items-center gap-x-2 text-xs text-muted-foreground">
                <span className="font-medium text-foreground">{passage.document_name}</span>
                {passage.page !== null && <span>p. {passage.page}</span>}
                {passage.section_path && <span className="truncate">{passage.section_path}</span>}
                {passage.kind === 'table' && <Badge variant="secondary">table</Badge>}
                {passage.topic_id && topicLabel.has(passage.topic_id) && (
                  <Badge variant="outline">{topicLabel.get(passage.topic_id)}</Badge>
                )}
                <span className="ml-auto tabular-nums">{Math.round(passage.score * 100)}%</span>
              </p>
              <p className="mt-1 line-clamp-4 text-[13px] break-words whitespace-pre-wrap">
                {passage.content}
              </p>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

function draftOf(settings: KnowledgeSettings) {
  return {
    url: settings.embed_base_url ?? '',
    model: settings.embed_base_url ? settings.embed_model : '',
    dims: settings.embed_dims?.toString() ?? '',
    passages: settings.passages.toString(),
    budget: settings.budget_chars.toString(),
    nodes: settings.use_in_nodes,
    plan: settings.use_in_plan,
  };
}

function SettingsForm({ workspace, stored }: { workspace: Workspace; stored: KnowledgeSettings }) {
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState(() => draftOf(stored));
  const [apiKey, setApiKey] = useState('');
  const canEdit = isWorkspaceAdmin(workspace.role);
  const save = useMutation({
    mutationFn: () => {
      const body: KnowledgeSettingsInput = {
        embed_base_url: draft.url.trim() || null,
        embed_model: draft.model.trim() || null,
        embed_dims: draft.dims ? Number(draft.dims) : null,
        passages: Number(draft.passages) || 0,
        budget_chars: Number(draft.budget) || 6000,
        use_in_nodes: draft.nodes,
        use_in_plan: draft.plan,
      };
      // Left out, the stored key stays as it is.
      if (apiKey) body.api_key = apiKey;
      return saveKnowledgeSettings(workspace.id, body);
    },
    meta: { errorToast: false, successMessage: 'Knowledge settings saved' },
    onSuccess: (saved) => {
      queryClient.setQueryData(qk.knowledge.settings(workspace.id), saved);
      setDraft(draftOf(saved));
      setApiKey('');
      void queryClient.invalidateQueries({ queryKey: qk.knowledge.all });
    },
  });
  const set = (patch: Partial<typeof draft>) => setDraft((d) => ({ ...d, ...patch }));
  const field = 'h-8 text-[13px]';
  const label = 'space-y-1 text-xs text-muted-foreground';

  return (
    <form
      className="space-y-4 rounded-lg border p-4"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate();
      }}
    >
      <div className="space-y-1">
        <h2 className="text-[13px] font-medium">Embedding and use</h2>
        <p className="text-[13px] text-muted-foreground">
          {stored.semantic
            ? `Passages are embedded with ${stored.embed_model}, so search understands meaning as well as words.`
            : 'No embedding model is set: search matches words, not meaning. Name an OpenAI-compatible embeddings endpoint to search by meaning.'}
        </p>
      </div>
      <fieldset disabled={!canEdit} className="space-y-4">
        <div className="grid gap-3 sm:grid-cols-2">
          <label className={label}>
            Embeddings endpoint
            <Input
              value={draft.url}
              placeholder="https://api.openai.com/v1"
              className={`${field} text-foreground`}
              onChange={(e) => set({ url: e.target.value })}
            />
          </label>
          <label className={label}>
            Model
            <Input
              value={draft.model}
              placeholder="text-embedding-3-small"
              className={`${field} text-foreground`}
              onChange={(e) => set({ model: e.target.value })}
            />
          </label>
          <label className={label}>
            API key {stored.has_api_key ? `(stored, ends in ${stored.key_hint ?? '…'})` : ''}
            <Input
              type="password"
              value={apiKey}
              autoComplete="off"
              placeholder={stored.has_api_key ? 'Leave empty to keep it' : 'Key of that endpoint'}
              className={`${field} text-foreground`}
              onChange={(e) => setApiKey(e.target.value)}
            />
          </label>
          <label className={label}>
            Vector size (optional)
            <Input
              inputMode="numeric"
              value={draft.dims}
              placeholder="Model default"
              className={`${field} text-foreground`}
              onChange={(e) => set({ dims: e.target.value.replace(/\D/g, '') })}
            />
          </label>
          <label className={label}>
            Passages per prompt (0 turns documents off)
            <Input
              inputMode="numeric"
              value={draft.passages}
              className={`${field} text-foreground`}
              onChange={(e) => set({ passages: e.target.value.replace(/\D/g, '') })}
            />
          </label>
          <label className={label}>
            Characters they may take together
            <Input
              inputMode="numeric"
              value={draft.budget}
              className={`${field} text-foreground`}
              onChange={(e) => set({ budget: e.target.value.replace(/\D/g, '') })}
            />
          </label>
        </div>
        <div className="flex flex-wrap gap-x-6 gap-y-2 text-[13px]">
          <label className="flex items-center gap-2">
            <Switch checked={draft.nodes} onCheckedChange={(nodes) => set({ nodes })} />
            Give passages to running nodes
          </label>
          <label className="flex items-center gap-2">
            <Switch checked={draft.plan} onCheckedChange={(plan) => set({ plan })} />
            Give passages to the planner
          </label>
        </div>
      </fieldset>
      {save.error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(save.error)}
        </p>
      )}
      {canEdit ? (
        <Button type="submit" size="sm" disabled={save.isPending}>
          Save
        </Button>
      ) : (
        <p className="text-xs text-muted-foreground">Only admins change these settings.</p>
      )}
    </form>
  );
}

function Knowledge({ workspace }: { workspace: Workspace }) {
  const { data: settings } = useQuery(knowledgeSettingsQuery(workspace.id));
  if (workspace.role === 'guest') {
    return (
      <EmptyState
        icon={BookOpenIcon}
        title="Guests do not see the workspace's documents"
        description="Ask a member of this workspace."
      />
    );
  }
  return (
    <div className="space-y-8">
      <Documents workspace={workspace} />
      <TrySearch workspace={workspace} />
      {settings ? (
        <SettingsForm key={JSON.stringify(settings)} workspace={workspace} stored={settings} />
      ) : (
        <Skeleton className="h-64 w-full rounded-lg" />
      )}
    </div>
  );
}

export default function KnowledgeSettingsPage() {
  const { current } = useCurrentWorkspace();
  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <Seo title="Knowledge" noIndex />
      <PageHeader
        title="Knowledge"
        description="Documents your workspace works from. They are split into passages, and the ones that bear on a task are given to the nodes and the planner, with their source."
      />
      {current ? <Knowledge key={current.id} workspace={current} /> : <PageSkeleton />}
    </div>
  );
}
