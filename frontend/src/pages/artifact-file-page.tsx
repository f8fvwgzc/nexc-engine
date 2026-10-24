import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CopyIcon,
  DownloadIcon,
  FileUpIcon,
  FolderInputIcon,
  HistoryIcon,
  MoreHorizontalIcon,
  PencilIcon,
  Trash2Icon,
} from 'lucide-react';
import { Fragment, useEffect, useRef, useState, type ReactNode } from 'react';
import { Link, useBlocker, useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { toast } from 'sonner';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { ErrorState } from '@/components/custom-ui/error-state';
import { IconButton } from '@/components/custom-ui/icon-button';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import {
  artifactContentQuery,
  artifactFileQuery,
  artifactFoldersQuery,
  artifactVersionsQuery,
  invalidateArtifacts,
  saveArtifactContent,
  updateArtifactFile,
} from '@/features/artifacts/api';
import { ArtifactGlyph, IssueChip } from '@/features/artifacts/artifact-glyphs';
import { ArtifactPreview, ArtifactSource } from '@/features/artifacts/artifact-view';
import {
  MAX_ARTIFACT_BYTES,
  fileNameCheck,
  folderPath,
  hasPreview,
} from '@/features/artifacts/files';
import { useFileActions } from '@/features/artifacts/use-artifact-actions';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { ApiError, errorMessage } from '@/lib/api/errors';
import { formatBytes, formatDateTime, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { ArtifactFile } from '@/schemas/artifact';

/** Where the folder a file is in opens in the browser. */
const folderLink = (folderId: string | null) => `/app/artifacts?folder=${folderId ?? 'root'}`;

/** The folders a file sits in, from All files down, each a link. */
function FolderTrail({ file, guest }: { file: ArtifactFile; guest: boolean }) {
  const { data: folders = [] } = useQuery({
    ...artifactFoldersQuery(file.workspace_id),
    enabled: !guest && file.folder_id !== null,
  });
  const path = file.folder_id ? folderPath(folders, file.folder_id) : [];
  const link =
    'rounded-sm outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50';
  return (
    <nav aria-label="Folder" className="flex min-w-0 items-center gap-1 truncate">
      <Link to="/app/artifacts" className={link}>
        All files
      </Link>
      {file.folder_id === null ? (
        <>
          <span aria-hidden>›</span>
          <Link to={folderLink(null)} className={link}>
            Not in a folder
          </Link>
        </>
      ) : (
        path.map((folder) => (
          <Fragment key={folder.id}>
            <span aria-hidden>›</span>
            <Link to={folderLink(folder.id)} className={cn(link, 'truncate')}>
              {folder.name}
            </Link>
          </Fragment>
        ))
      )}
    </nav>
  );
}

/** The file's name as the page's heading; writers click it to rename the file in place. */
function FileName({ file, canWrite }: { file: ArtifactFile; canWrite: boolean }) {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  // Escape leaves the field without saving what was typed.
  const cancelled = useRef(false);
  const rename = useMutation({
    mutationFn: (name: string) => updateArtifactFile(file.id, { name }),
    onSuccess: (renamed) => {
      queryClient.setQueryData(qk.artifacts.file(renamed.id), renamed);
      invalidateArtifacts(queryClient, renamed.workspace_id, renamed.issue?.id);
    },
  });
  const commit = (value: string) => {
    setEditing(false);
    const name = value.trim();
    if (name === file.name) return;
    const problem = fileNameCheck(name);
    if (problem) toast.error(problem);
    else rename.mutate(name);
  };
  if (editing) {
    return (
      <>
        {/* The name is edited in place; the heading still names the page. */}
        <h1 className="sr-only">{file.name}</h1>
        <Input
          autoFocus
          defaultValue={file.name}
          aria-label="File name"
          className="h-8 max-w-md font-semibold"
          onFocus={(e) => {
            const dot = e.target.value.lastIndexOf('.');
            e.target.setSelectionRange(0, dot > 0 ? dot : e.target.value.length);
          }}
          onBlur={(e) => {
            if (cancelled.current) {
              cancelled.current = false;
              setEditing(false);
            } else {
              commit(e.target.value);
            }
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur();
            if (e.key === 'Escape') {
              e.stopPropagation();
              cancelled.current = true;
              e.currentTarget.blur();
            }
          }}
        />
      </>
    );
  }
  return (
    <h1 className="flex min-w-0 items-center gap-2 text-lg font-semibold tracking-tight">
      <ArtifactGlyph kind={file.kind} className="size-4" />
      {canWrite ? (
        <button
          type="button"
          title="Rename"
          onClick={() => setEditing(true)}
          className="min-w-0 truncate rounded-sm text-left outline-none hover:underline hover:decoration-muted-foreground/50 hover:underline-offset-4 focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          {rename.isPending ? (rename.variables ?? file.name) : file.name}
        </button>
      ) : (
        <span className="min-w-0 truncate">{file.name}</span>
      )}
    </h1>
  );
}

/** The saved versions, newest first: who saved each, when and how large; one opens on click. */
function Versions({
  file,
  viewing,
  onView,
  disabled,
}: {
  file: ArtifactFile;
  viewing: number;
  onView: (version: number) => void;
  disabled: boolean;
}) {
  const { data: versions, isPending } = useQuery(artifactVersionsQuery(file.id));
  return (
    <aside
      aria-label="Versions"
      className="space-y-2 border-t pt-4 lg:w-64 lg:shrink-0 lg:border-t-0 lg:border-l lg:pt-0 lg:pl-4"
    >
      <h2 className="flex items-center gap-1.5 text-[13px] font-medium text-muted-foreground">
        <HistoryIcon className="size-3.5" aria-hidden />
        Versions
        <span className="tabular-nums">{file.current_version}</span>
      </h2>
      {isPending ? (
        <Skeleton className="h-24" />
      ) : (
        <ol className="space-y-0.5">
          {versions?.map((v) => (
            <li key={v.version}>
              <button
                type="button"
                disabled={disabled}
                aria-current={v.version === viewing ? 'true' : undefined}
                onClick={() => onView(v.version)}
                className="flex w-full flex-col rounded-md px-2 py-1.5 text-left text-[13px] outline-none hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-60 aria-[current]:bg-muted"
              >
                <span className="flex items-center gap-2">
                  <span className="font-medium tabular-nums">v{v.version}</span>
                  {v.version === file.current_version && (
                    <Badge variant="outline" className="h-4 px-1 text-[10px]">
                      current
                    </Badge>
                  )}
                  <span className="ml-auto text-xs text-muted-foreground tabular-nums">
                    {formatBytes(v.size)}
                  </span>
                </span>
                <span
                  className="truncate text-xs text-muted-foreground"
                  title={formatDateTime(v.created_at)}
                >
                  {v.created_by?.name ?? 'Someone'} · {formatRelative(v.created_at)}
                </span>
              </button>
            </li>
          ))}
        </ol>
      )}
    </aside>
  );
}

interface Draft {
  /** The version the edit started from: saving over any other is a conflict. */
  base: number;
  /** The file's text as it was loaded, line endings and all. */
  original: string;
  /** The text as the edit started, to tell whether anything was changed. */
  start: string;
  text: string;
  /**
   * Whether every line of the file ends in CRLF. A textarea only ever gives back LF, so the CRs
   * are put back on save; otherwise one changed character would rewrite every line ending. A file
   * that mixes its line endings is saved with LF once it is changed.
   */
  crlf: boolean;
}

/** Whether the text has line breaks and every one is CRLF (no lone CR, no lone LF). */
const allCrlf = (content: string) =>
  content.includes('\r\n') && !/\r(?!\n)|(?<!\r)\n/.test(content);

function startDraft(base: number, content: string): Draft {
  const text = content.replace(/\r\n?/g, '\n');
  return { base, original: content, start: text, text, crlf: allCrlf(content) };
}

/** Whether the text was changed since the edit started. */
const edited = (draft: Draft) => draft.text !== draft.start;

/**
 * The bytes a draft saves as: the file's own while nothing was changed, otherwise the text with
 * CRLF put back when every line had it.
 */
const draftBody = (draft: Draft) =>
  !edited(draft) ? draft.original : draft.crlf ? draft.text.replace(/\n/g, '\r\n') : draft.text;

/** Said when a save made no new version: the content is the current version's. */
const NOTHING_CHANGED = 'Nothing changed, so no new version was saved';

/** Puts the text being edited on the clipboard, for when it cannot be saved here. */
function copyText(text: string) {
  void navigator.clipboard
    .writeText(text)
    .then(() => toast.success('Your text is on the clipboard'))
    .catch(() => toast.error('Couldn’t copy the text'));
}

/** What a save sent: the version it went over and the bytes. */
interface Sent {
  base: number;
  body: string;
}

/**
 * The text being edited, with Save (⌘S) and Cancel, and what to do when someone saved first.
 * Saving with nothing changed asks nothing of the server: no new version is made.
 */
function Editor({
  file,
  draft,
  onChange,
  onCancel,
  onSaved,
  onUnchanged,
  onReload,
}: {
  file: ArtifactFile;
  draft: Draft;
  onChange: (text: string) => void;
  onCancel: () => void;
  onSaved: (saved: ArtifactFile, base: number, text: string) => void;
  onUnchanged: () => void;
  onReload: () => Promise<void>;
}) {
  const queryClient = useQueryClient();
  const body = draftBody(draft);
  // What was sent travels with the save: its answer records those bytes, not the text by then.
  const save = useMutation({
    mutationFn: (sent: Sent) => saveArtifactContent(file.id, sent.base, sent.body),
    meta: { errorToast: false },
    onSuccess: (saved, sent) => onSaved(saved, sent.base, sent.body),
    // Someone saved first: fetch the file again, so the message can say which version they made.
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: qk.artifacts.file(file.id) });
      }
    },
  });
  const conflict = save.error instanceof ApiError && save.error.status === 409;
  const submit = () => {
    if (save.isPending) return;
    if (edited(draft)) save.mutate({ base: draft.base, body });
    else onUnchanged();
  };
  return (
    <form
      className="space-y-3"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <Textarea
        autoFocus
        value={draft.text}
        // Held while saving: what is typed then would be in no version.
        readOnly={save.isPending}
        spellCheck={false}
        aria-label={`Content of ${file.name}`}
        className="min-h-[60svh] resize-y font-mono text-[13px] leading-relaxed"
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
            e.preventDefault();
            submit();
          }
        }}
      />
      {conflict ? (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-2 rounded-md border border-destructive/30 bg-destructive/[0.04] px-3 py-2 text-[13px]"
        >
          <span className="min-w-0 flex-1">
            {file.current_version > draft.base
              ? `Someone else saved v${file.current_version} first.`
              : 'Someone else saved a newer version first.'}{' '}
            Reload to edit theirs; your text here is replaced, so copy it first if you need it.
          </span>
          <Button type="button" variant="outline" size="sm" onClick={() => copyText(draft.text)}>
            <CopyIcon />
            Copy my text
          </Button>
          <Button
            type="button"
            size="sm"
            onClick={() =>
              void onReload().then(
                () => save.reset(),
                (e: unknown) => toast.error(errorMessage(e)),
              )
            }
          >
            Reload
          </Button>
        </div>
      ) : (
        save.error && (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(save.error)}
          </p>
        )
      )}
      <div className="flex items-center gap-2">
        <Button type="submit" size="sm" disabled={save.isPending || conflict}>
          Save as v{draft.base + 1}
        </Button>
        <Button type="button" variant="outline" size="sm" onClick={onCancel}>
          Cancel
        </Button>
        <span className="ml-auto text-xs text-muted-foreground tabular-nums">
          {formatBytes(new Blob([body]).size)} of {formatBytes(MAX_ARTIFACT_BYTES)}
        </span>
      </div>
    </form>
  );
}

/**
 * One file: its name, where it is, its issue, the version shown, and that version's content.
 * `gone` says the file was deleted (or put out of reach) while open: what is on screen stays,
 * so a draft can still be copied.
 */
function FileView({ file, gone }: { file: ArtifactFile; gone: boolean }) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const { all } = useCurrentWorkspace();
  const role = all.find((w) => w.id === file.workspace_id)?.role;
  const guest = role === 'guest';
  const canWrite = role !== undefined && !guest;
  const asked = Number(params.get('version'));
  const viewing =
    Number.isInteger(asked) && asked > 0 && asked <= file.current_version
      ? asked
      : file.current_version;
  const current = viewing === file.current_version;
  const content = useQuery(artifactContentQuery(file.id, viewing));
  const [draft, setDraft] = useState<Draft | null>(null);
  const [tab, setTab] = useState(hasPreview(file) ? 'preview' : 'source');
  const upload = useRef<HTMLInputElement>(null);
  const actions = useFileActions(file.workspace_id, {
    onDeleted: (gone) => void navigate(folderLink(gone.folder_id), { replace: true }),
    onDuplicated: (copy) => void navigate(`/app/artifacts/${copy.id}`),
  });

  const view = (version: number | null) =>
    setParams(
      (next) => {
        if (version === null || version === file.current_version) next.delete('version');
        else next.set('version', String(version));
        return next;
      },
      { replace: true },
    );

  // A new file opens ready to write in (`?edit=1`).
  const wantsEdit = params.get('edit') === '1';
  if (wantsEdit && draft === null && canWrite && current && content.data) {
    setDraft(startDraft(file.current_version, content.data.content));
  }
  useEffect(() => {
    if (wantsEdit && draft !== null) {
      setParams(
        (next) => {
          next.delete('edit');
          return next;
        },
        { replace: true },
      );
    }
  }, [wantsEdit, draft, setParams]);

  // Unsaved text is not left behind without asking: not for another page, not for the tab closing.
  const dirty = draft !== null && draft.text !== draft.start;
  const blocker = useBlocker(
    ({ currentLocation, nextLocation }) =>
      dirty && currentLocation.pathname !== nextLocation.pathname,
  );
  const leaving = useRef(false);
  useEffect(() => {
    if (!dirty) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  const saved = (next: ArtifactFile, base: number, text?: string) => {
    queryClient.setQueryData(qk.artifacts.file(next.id), next);
    if (text !== undefined) {
      queryClient.setQueryData(qk.artifacts.content(next.id, next.current_version), {
        file_id: next.id,
        version: next.current_version,
        content: text,
      });
    }
    void queryClient.invalidateQueries({ queryKey: qk.artifacts.versions(next.id) });
    invalidateArtifacts(queryClient, next.workspace_id, next.issue?.id);
    setDraft(null);
    view(null);
    toast.success(
      next.current_version === base ? NOTHING_CHANGED : `Saved v${next.current_version}`,
    );
  };

  const reload = async () => {
    const fresh = await queryClient.fetchQuery({ ...artifactFileQuery(file.id), staleTime: 0 });
    const latest = await queryClient.fetchQuery(
      artifactContentQuery(fresh.id, fresh.current_version),
    );
    void queryClient.invalidateQueries({ queryKey: qk.artifacts.versions(file.id) });
    setDraft(startDraft(fresh.current_version, latest.content));
  };

  const replace = useMutation({
    mutationFn: ({ body, base }: { body: Blob | string; base: number }) =>
      saveArtifactContent(file.id, base, body),
    onSuccess: (next, { body, base }) =>
      saved(next, base, typeof body === 'string' ? body : undefined),
    // Someone saved first: fetch the file again, so a retry goes over their version.
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409) {
        void queryClient.invalidateQueries({ queryKey: qk.artifacts.file(file.id) });
      }
    },
  });
  const uploadVersion = (picked: File | undefined) => {
    if (!picked) return;
    if (picked.size > MAX_ARTIFACT_BYTES) {
      toast.error(`${picked.name} is larger than 1 MB, the most a file may hold`);
      return;
    }
    replace.mutate({ body: picked, base: file.current_version });
  };

  const startEdit = () => {
    if (content.data && current) setDraft(startDraft(file.current_version, content.data.content));
  };

  let body: ReactNode;
  if (draft) {
    body = (
      <Editor
        file={file}
        draft={draft}
        onChange={(text) => setDraft({ ...draft, text })}
        onCancel={() => setDraft(null)}
        onSaved={saved}
        onUnchanged={() => {
          setDraft(null);
          toast.success(NOTHING_CHANGED);
        }}
        onReload={reload}
      />
    );
  } else if (content.isPending) {
    body = (
      <div role="status" aria-busy aria-label="Loading the file">
        <Skeleton className="h-72 rounded-lg" />
      </div>
    );
  } else if (content.isError) {
    body = (
      <ErrorState
        compact
        title="Couldn’t load this version"
        error={content.error}
        onRetry={() => void content.refetch()}
      />
    );
  } else if (content.data.content === '') {
    body = (
      <p className="rounded-lg border border-dashed px-4 py-10 text-center text-[13px] text-muted-foreground">
        This file is empty.{canWrite && current && ' Edit it to write the first lines.'}
      </p>
    );
  } else if (hasPreview(file)) {
    body = (
      <Tabs value={tab} onValueChange={setTab} className="gap-3">
        <TabsList variant="line">
          <TabsTrigger value="preview">Preview</TabsTrigger>
          <TabsTrigger value="source">Source</TabsTrigger>
        </TabsList>
        <TabsContent value="preview">
          <ArtifactPreview file={file} content={content.data.content} />
        </TabsContent>
        <TabsContent value="source">
          <ArtifactSource content={content.data.content} language={file.language} />
        </TabsContent>
      </Tabs>
    );
  } else {
    body = <ArtifactSource content={content.data.content} language={file.language} />;
  }

  const versionOptions = Array.from({ length: file.current_version }, (_, i) => {
    const v = file.current_version - i;
    return { value: String(v), label: v === file.current_version ? `v${v} · current` : `v${v}` };
  });

  return (
    <PageContainer className="max-w-none">
      <Seo title={file.name} noIndex />
      <header className="space-y-2 border-b pb-4">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <div className="min-w-0 flex-1">
            <FileName file={file} canWrite={canWrite} />
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            {canWrite && (
              <Button
                size="sm"
                variant="outline"
                disabled={draft !== null || !current || !content.data}
                onClick={startEdit}
              >
                <PencilIcon />
                Edit
              </Button>
            )}
            <IconButton
              label={current ? `Download ${file.name}` : `Download v${viewing}`}
              onClick={() => actions.download(file, current ? undefined : viewing)}
            >
              <DownloadIcon />
            </IconButton>
            {canWrite && (
              <>
                <IconButton
                  label="Upload new version"
                  disabled={draft !== null || replace.isPending}
                  onClick={() => upload.current?.click()}
                >
                  <FileUpIcon />
                </IconButton>
                <input
                  ref={upload}
                  type="file"
                  className="sr-only"
                  tabIndex={-1}
                  aria-label={`New version of ${file.name}`}
                  onChange={(e) => {
                    uploadVersion(e.target.files?.[0]);
                    e.target.value = '';
                  }}
                />
                <DropdownMenu>
                  {/* Shut while editing: duplicating or deleting would drop the draft. */}
                  <DropdownMenuTrigger asChild disabled={draft !== null}>
                    <IconButton label={`More actions for ${file.name}`}>
                      <MoreHorizontalIcon />
                    </IconButton>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem onSelect={() => actions.duplicate(file)}>
                      <CopyIcon />
                      Duplicate
                    </DropdownMenuItem>
                    <DropdownMenuItem onSelect={() => actions.move(file)}>
                      <FolderInputIcon />
                      Move…
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem variant="destructive" onSelect={() => actions.remove(file)}>
                      <Trash2Icon />
                      Delete
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </>
            )}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-[13px] text-muted-foreground">
          <FolderTrail file={file} guest={guest} />
          {file.issue && <IssueChip issue={file.issue} />}
          <OptionSelect
            value={String(viewing)}
            onValueChange={(v) => view(Number(v))}
            options={versionOptions}
            disabled={draft !== null}
            aria-label="Version"
            className="h-7 w-32 text-xs"
          />
          <span className="tabular-nums">{formatBytes(file.size)}</span>
          <span title={formatDateTime(file.updated_at)}>
            Updated {formatRelative(file.updated_at)}
            {file.updated_by && ` by ${file.updated_by.name}`}
          </span>
        </div>
      </header>
      {gone && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-2 rounded-md border border-destructive/30 bg-destructive/[0.04] px-3 py-2 text-[13px]"
        >
          <span className="min-w-0 flex-1">
            This file was deleted, or you can no longer see it.
            {draft && ' Your text here cannot be saved; copy it before you leave.'}
          </span>
          {draft && (
            <Button size="sm" variant="outline" onClick={() => copyText(draft.text)}>
              <CopyIcon />
              Copy my text
            </Button>
          )}
          <Button size="sm" variant="ghost" asChild>
            <Link to="/app/artifacts">All files</Link>
          </Button>
        </div>
      )}
      {!current && !draft && (
        <div className="flex flex-wrap items-center gap-2 rounded-md border bg-muted/40 px-3 py-2 text-[13px]">
          <span className="min-w-0 flex-1">
            This is v{viewing}, an older version: it can be read, not changed.
          </span>
          {canWrite && content.data && (
            <Button
              size="sm"
              variant="outline"
              disabled={replace.isPending}
              onClick={() => {
                if (content.data) {
                  replace.mutate({ body: content.data.content, base: file.current_version });
                }
              }}
            >
              Restore as v{file.current_version + 1}
            </Button>
          )}
          <Button size="sm" variant="ghost" onClick={() => view(null)}>
            Back to v{file.current_version}
          </Button>
        </div>
      )}
      <div className="flex flex-col gap-6 lg:flex-row">
        <div className="min-w-0 flex-1">{body}</div>
        <Versions file={file} viewing={viewing} onView={view} disabled={draft !== null} />
      </div>
      {actions.dialogs}
      <ConfirmDialog
        open={blocker.state === 'blocked'}
        onOpenChange={(open) => {
          if (!open && !leaving.current) blocker.reset?.();
        }}
        title="Leave without saving?"
        description={`Your changes to ${file.name} are not saved and will be lost.`}
        confirmLabel="Leave"
        destructive
        onConfirm={() => {
          leaving.current = true;
          blocker.proceed?.();
        }}
      />
    </PageContainer>
  );
}

function ArtifactFileLoader({ fileId }: { fileId: string }) {
  const { data: file, isPending, error, refetch } = useQuery(artifactFileQuery(fileId));
  if (isPending) return <PageSkeleton />;
  const gone = error instanceof ApiError && error.status === 404;
  // A failed refetch keeps the file on screen (and any draft with it); only a first load fails here.
  if (file === undefined) {
    return (
      <PageContainer className="max-w-none">
        <ErrorState
          headingLevel={1}
          title={gone ? 'This file is not here' : 'Couldn’t load the file'}
          error={error}
          description={
            gone ? 'It was deleted, or it belongs to an issue you cannot see.' : undefined
          }
          onRetry={gone ? undefined : () => void refetch()}
          action={
            <Button variant="outline" asChild>
              <Link to="/app/artifacts">All files</Link>
            </Button>
          }
        />
      </PageContainer>
    );
  }
  return <FileView file={file} gone={gone} />;
}

export default function ArtifactFilePage() {
  const { fileId = '' } = useParams();
  return (
    <>
      <Seo title="File" noIndex />
      <ArtifactFileLoader key={fileId} fileId={fileId} />
    </>
  );
}
