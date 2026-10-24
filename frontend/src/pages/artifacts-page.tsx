import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CopyIcon,
  DownloadIcon,
  ExternalLinkIcon,
  FilePlusIcon,
  FolderIcon,
  FolderInputIcon,
  FolderOpenIcon,
  FolderPlusIcon,
  FolderUpIcon,
  MoreHorizontalIcon,
  PencilIcon,
  SearchIcon,
  Trash2Icon,
  UploadIcon,
} from 'lucide-react';
import { Fragment, useRef, useState, type DragEvent, type ReactNode } from 'react';
import { Link, useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { toast } from 'sonner';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { IconButton } from '@/components/custom-ui/icon-button';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton, TableSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Spinner } from '@/components/ui/spinner';
import {
  artifactFilesQuery,
  artifactFoldersQuery,
  createArtifactFolder,
  invalidateArtifacts,
  updateArtifactFile,
  updateArtifactFolder,
  uploadArtifactFile,
  type ArtifactFileFilter,
} from '@/features/artifacts/api';
import { NameDialog } from '@/features/artifacts/artifact-dialogs';
import { ArtifactGlyph, IssueChip } from '@/features/artifacts/artifact-glyphs';
import {
  buildFolderTree,
  fileNameCheck,
  folderAndDescendants,
  folderNameProblem,
  folderPath,
  sortByName,
} from '@/features/artifacts/files';
import { useFileActions, useFolderActions } from '@/features/artifacts/use-artifact-actions';
import {
  dragHasFiles,
  entriesFromDrop,
  entriesFromFiles,
  useArtifactUpload,
} from '@/features/artifacts/use-artifact-upload';
import { issuesQuery } from '@/features/issues/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { errorMessage } from '@/lib/api/errors';
import { formatBytes, formatDateTime, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { ArtifactFile, ArtifactFolder } from '@/schemas/artifact';
import type { Workspace } from '@/schemas/workspace';

const ROOT = 'root';
const ANY = 'any';

/** Navigation state of a change to the issue filter, which keeps the search typed. */
interface BrowserNavState {
  keepSearch?: boolean;
}

/** A row being dragged: what it is, where it is now, and the folders it cannot go into. */
interface Dragged {
  kind: 'file' | 'folder';
  id: string;
  name: string;
  from: string | null;
  refuse: Set<string>;
}

/** Marks a drag as a row of this page, apart from files dragged in from the desktop. */
const ROW_TYPE = 'application/x-nexc-artifact';

/**
 * Moving rows by dragging: `source` makes a row draggable, `target` makes a folder row or a
 * crumb take it (null: the top). A row is not taken where it already is, nor a folder into
 * itself or a folder under it.
 */
function useRowDrag(onMove: (row: Dragged, to: string | null) => void) {
  const [dragging, setDragging] = useState<Dragged | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const allowed = (row: Dragged, to: string | null) =>
    row.from !== to && !(to !== null && row.refuse.has(to));
  const end = () => {
    setDragging(null);
    setOver(null);
  };
  return {
    source: (row: Omit<Dragged, 'refuse'>, refuse = () => new Set<string>()) => ({
      draggable: true,
      onDragStart: (e: DragEvent) => {
        e.dataTransfer.setData(ROW_TYPE, row.id);
        e.dataTransfer.effectAllowed = 'move';
        setDragging({ ...row, refuse: refuse() });
      },
      onDragEnd: end,
    }),
    target: (to: string | null) => {
      const key = to ?? ROOT;
      return {
        'data-drop': over === key || undefined,
        onDragOver: (e: DragEvent) => {
          if (!dragging || !allowed(dragging, to)) return;
          e.preventDefault();
          e.stopPropagation();
          e.dataTransfer.dropEffect = 'move';
          setOver(key);
        },
        onDragLeave: () => setOver((current) => (current === key ? null : current)),
        onDrop: (e: DragEvent) => {
          if (!dragging) return;
          e.preventDefault();
          e.stopPropagation();
          if (allowed(dragging, to)) onMove(dragging, to);
          end();
        },
      };
    },
  };
}

type RowDrag = ReturnType<typeof useRowDrag>;
type DropTarget = ReturnType<RowDrag['target']>;

const CELL = 'shrink-0 text-xs text-muted-foreground tabular-nums';

/** One folder of the table: opens on click, takes rows dropped on it, and has its menu. */
function FolderRow({
  folder,
  drag,
  drop,
  onRename,
  onMove,
  onDelete,
}: {
  folder: ArtifactFolder;
  /** Set for writers: the row can be dragged, and opens a menu. */
  drag?: ReturnType<RowDrag['source']>;
  drop: DropTarget;
  onRename: () => void;
  onMove: () => void;
  onDelete: () => void;
}) {
  return (
    <li
      {...drag}
      {...drop}
      className="relative flex h-9 items-center gap-3 px-3 text-[13px] hover:bg-muted/60 data-[drop]:bg-brand/10 data-[drop]:ring-2 data-[drop]:ring-brand/40 data-[drop]:ring-inset"
    >
      <span className="flex min-w-0 flex-1 items-center gap-2">
        <FolderIcon className="size-3.5 shrink-0 fill-muted-foreground/20 text-muted-foreground" />
        <Link
          to={`/app/artifacts?folder=${folder.id}`}
          draggable={false}
          className="truncate font-medium outline-none after:absolute after:inset-0 focus-visible:underline"
        >
          {folder.name}
        </Link>
      </span>
      <span className={cn(CELL, 'hidden w-24 sm:block')} />
      <span className={cn(CELL, 'w-12')} />
      <span className={cn(CELL, 'hidden w-16 text-right md:block')} />
      <span
        className={cn(CELL, 'hidden w-44 truncate md:block')}
        title={formatDateTime(folder.updated_at)}
      >
        {formatRelative(folder.updated_at)}
      </span>
      <span className="relative z-10 w-7">
        {drag && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <IconButton label={`Actions for ${folder.name}`} size="icon-xs">
                <MoreHorizontalIcon />
              </IconButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem asChild>
                <Link to={`/app/artifacts?folder=${folder.id}`}>
                  <FolderOpenIcon />
                  Open
                </Link>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onRename}>
                <PencilIcon />
                Rename
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onMove}>
                <FolderInputIcon />
                Move…
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onSelect={onDelete}>
                <Trash2Icon />
                Delete
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </span>
    </li>
  );
}

/** One file of the table: its kind, issue, version, size and last change, and its menu. */
function FileRow({
  file,
  drag,
  actions,
}: {
  file: ArtifactFile;
  /** Set for writers: the row can be dragged, and its menu changes the file. */
  drag?: ReturnType<RowDrag['source']>;
  actions: ReturnType<typeof useFileActions>;
}) {
  const canWrite = drag !== undefined;
  return (
    <li
      {...drag}
      className="relative flex h-9 items-center gap-3 px-3 text-[13px] hover:bg-muted/60"
    >
      <span className="flex min-w-0 flex-1 items-center gap-2">
        <ArtifactGlyph kind={file.kind} />
        <Link
          to={`/app/artifacts/${file.id}`}
          draggable={false}
          title={file.name}
          className="truncate outline-none after:absolute after:inset-0 focus-visible:underline"
        >
          {file.name}
        </Link>
      </span>
      <span className="relative z-10 hidden w-24 sm:block">
        {file.issue && <IssueChip issue={file.issue} />}
      </span>
      <span className={cn(CELL, 'w-12')}>v{file.current_version}</span>
      <span className={cn(CELL, 'hidden w-16 text-right md:block')}>{formatBytes(file.size)}</span>
      <span
        className={cn(CELL, 'hidden w-44 truncate md:block')}
        title={formatDateTime(file.updated_at)}
      >
        {formatRelative(file.updated_at)}
        {file.updated_by && ` · ${file.updated_by.name}`}
      </span>
      <span className="relative z-10 w-7">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <IconButton label={`Actions for ${file.name}`} size="icon-xs">
              <MoreHorizontalIcon />
            </IconButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem asChild>
              <Link to={`/app/artifacts/${file.id}`}>
                <ExternalLinkIcon />
                Open
              </Link>
            </DropdownMenuItem>
            {canWrite && (
              <>
                <DropdownMenuItem onSelect={() => actions.rename(file)}>
                  <PencilIcon />
                  Rename
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => actions.move(file)}>
                  <FolderInputIcon />
                  Move…
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => actions.duplicate(file)}>
                  <CopyIcon />
                  Duplicate
                </DropdownMenuItem>
              </>
            )}
            <DropdownMenuItem onSelect={() => actions.download(file)}>
              <DownloadIcon />
              Download
            </DropdownMenuItem>
            {canWrite && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onSelect={() => actions.remove(file)}>
                  <Trash2Icon />
                  Delete
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </span>
    </li>
  );
}

/** The folder trail above the table; each crumb opens its folder and takes dropped rows. */
function Trail({
  path,
  last,
  drop,
}: {
  path: ArtifactFolder[];
  /** A last crumb that is not a folder: "Not in a folder", search results. */
  last?: string;
  drop: RowDrag['target'];
}) {
  const crumb =
    'truncate rounded-sm px-1 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 data-[drop]:bg-brand/10 data-[drop]:text-foreground data-[drop]:ring-2 data-[drop]:ring-brand/40';
  const atTop = path.length === 0 && !last;
  return (
    <nav aria-label="Folders" className="flex min-w-0 items-center gap-1 text-[13px]">
      {atTop ? (
        <span aria-current="page" className="px-1 font-medium" {...drop(null)}>
          All files
        </span>
      ) : (
        <Link to="/app/artifacts" className={cn(crumb, 'text-muted-foreground')} {...drop(null)}>
          All files
        </Link>
      )}
      {path.map((folder, i) => (
        <Fragment key={folder.id}>
          <span aria-hidden className="text-muted-foreground">
            ›
          </span>
          {i === path.length - 1 && !last ? (
            <span aria-current="page" className="truncate px-1 font-medium">
              {folder.name}
            </span>
          ) : (
            <Link
              to={`/app/artifacts?folder=${folder.id}`}
              className={cn(crumb, 'text-muted-foreground')}
              {...drop(folder.id)}
            >
              {folder.name}
            </Link>
          )}
        </Fragment>
      ))}
      {last && (
        <>
          <span aria-hidden className="text-muted-foreground">
            ›
          </span>
          <span aria-current="page" className="truncate px-1 font-medium">
            {last}
          </span>
        </>
      )}
    </nav>
  );
}

type Asking = 'folder' | 'file' | null;

/**
 * The workspace's artifacts: folders first, then files, in the open folder, or every file that
 * matches a search or belongs to an issue. Files and whole folders dropped from the desktop are
 * uploaded into the open folder; a row dragged onto a folder or a crumb moves there.
 */
function Browser({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const location = useLocation();
  const [params, setParams] = useSearchParams();
  const guest = workspace.role === 'guest';
  const canWrite = !guest;
  const folderParam = params.get('folder');
  const issueId = params.get('issue');
  const [q, setQ] = useState('');
  // Opening a folder, a crumb or All files (from the table or the sidebar) leaves the search;
  // only a change of the issue filter keeps it.
  const [seenKey, setSeenKey] = useState(location.key);
  if (seenKey !== location.key) {
    setSeenKey(location.key);
    if (!(location.state as BrowserNavState | null)?.keepSearch) setQ('');
  }
  const debounced = useDebouncedValue(q.trim(), 250);
  // A cleared box shows the folder at once, without waiting out the debounce.
  const search = q.trim() === '' ? '' : debounced;
  const [asking, setAsking] = useState<Asking>(null);
  const [overPage, setOverPage] = useState(false);
  const filesInput = useRef<HTMLInputElement>(null);
  const folderInput = useRef<HTMLInputElement>(null);

  const foldersQuery = useQuery({ ...artifactFoldersQuery(workspace.id), enabled: !guest });
  const folders = foldersQuery.data ?? [];
  const openFolder =
    folderParam && folderParam !== ROOT ? folders.find((f) => f.id === folderParam) : undefined;
  const folderId = folderParam && folderParam !== ROOT ? folderParam : null;
  // A search or an issue looks across folders; a guest sees no folders, only the files they may.
  const filtering = Boolean(search || issueId);
  const across = filtering || guest;
  const filter: ArtifactFileFilter = across
    ? { q: search || undefined, issue_id: issueId ?? undefined }
    : folderId
      ? { folder_id: folderId }
      : { root: true };
  const files = useQuery(artifactFilesQuery(workspace.id, filter));
  const { data: issues = [] } = useQuery(issuesQuery(workspace.id));

  const fileActions = useFileActions(workspace.id);
  const folderActions = useFolderActions(workspace.id, {
    // Deleting the open folder, or one it is in, goes up to where that folder was.
    onDeleted: (gone) => {
      if (folderId && folderPath(folders, folderId).some((f) => f.id === gone.id)) {
        void navigate(`/app/artifacts?folder=${gone.parent_id ?? ROOT}`, { replace: true });
      }
    },
  });
  const uploads = useArtifactUpload(workspace.id, { folderId, issueId });

  const move = useMutation({
    mutationFn: async ({ row, to }: { row: Dragged; to: string | null }) => {
      if (row.kind === 'file') {
        const moved = await updateArtifactFile(row.id, { folder_id: to });
        queryClient.setQueryData(qk.artifacts.file(moved.id), moved);
      } else {
        await updateArtifactFolder(workspace.id, row.id, { parent_id: to });
      }
    },
    onSuccess: (_, { row, to }) => {
      invalidateArtifacts(queryClient, workspace.id);
      const where = to ? (folders.find((f) => f.id === to)?.name ?? 'the folder') : 'the top';
      toast.success(`Moved ${row.name} to ${where}`);
    },
  });
  const rows = useRowDrag((row, to) => move.mutate({ row, to }));

  const path = folderId ? folderPath(folders, folderId) : [];
  const shownFolders = across
    ? search && !issueId
      ? sortByName(folders.filter((f) => f.name.toLowerCase().includes(search.toLowerCase())))
      : []
    : folderParam === ROOT
      ? []
      : folderId
        ? sortByName(folders.filter((f) => f.parent_id === folderId))
        : buildFolderTree(folders).map((node) => node.folder);
  const issue = issueId ? issues.find((i) => i.id === issueId) : undefined;
  const lastCrumb = issueId
    ? `Files of ${issue?.identifier ?? 'an issue'}`
    : search
      ? 'Search results'
      : folderParam === ROOT
        ? 'Not in a folder'
        : undefined;
  const missing = folderId !== null && foldersQuery.isSuccess && !openFolder;

  const setIssue = (value: string) =>
    setParams(
      (next) => {
        if (value === ANY) next.delete('issue');
        else next.set('issue', value);
        return next;
      },
      { state: { keepSearch: true } satisfies BrowserNavState },
    );

  const createFolder = async (name: string) => {
    await createArtifactFolder(workspace.id, { name, parent_id: folderId });
    invalidateArtifacts(queryClient, workspace.id);
    toast.success(`Folder ${name} made`);
  };
  const createFile = async (name: string) => {
    const file = await uploadArtifactFile(
      workspace.id,
      { name, folder_id: folderId, issue_id: issueId },
      new Blob([]),
    );
    invalidateArtifacts(queryClient, workspace.id, issueId);
    void navigate(`/app/artifacts/${file.id}?edit=1`);
  };

  const dropFiles = {
    onDragOver: (e: DragEvent) => {
      if (!canWrite || !dragHasFiles(e.dataTransfer)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
      setOverPage(true);
    },
    onDragLeave: (e: DragEvent) => {
      if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setOverPage(false);
    },
    onDrop: (e: DragEvent) => {
      setOverPage(false);
      if (!canWrite || !dragHasFiles(e.dataTransfer)) return;
      e.preventDefault();
      // A drop during an upload waits its turn in the hook.
      entriesFromDrop(e.dataTransfer)
        .then(uploads.upload)
        .catch((error: unknown) => toast.error(errorMessage(error)));
    },
  };

  const header = canWrite && (
    <>
      <Button size="sm" variant="outline" onClick={() => setAsking('folder')} disabled={missing}>
        <FolderPlusIcon />
        New folder
      </Button>
      <Button size="sm" variant="outline" onClick={() => setAsking('file')} disabled={missing}>
        <FilePlusIcon />
        New file
      </Button>
      <Button
        size="sm"
        variant="outline"
        onClick={() => folderInput.current?.click()}
        disabled={missing || uploads.busy}
      >
        <FolderUpIcon />
        Upload folder
      </Button>
      <Button
        size="sm"
        onClick={() => filesInput.current?.click()}
        disabled={missing || uploads.busy}
      >
        <UploadIcon />
        Upload files
      </Button>
      <input
        ref={filesInput}
        type="file"
        multiple
        tabIndex={-1}
        className="sr-only"
        aria-label="Files to upload"
        onChange={(e) => {
          uploads.upload(entriesFromFiles(e.target.files ?? []));
          e.target.value = '';
        }}
      />
      <input
        ref={(input) => {
          folderInput.current = input;
          // `webkitdirectory` is not in React's types; every current browser supports it.
          input?.setAttribute('webkitdirectory', '');
        }}
        type="file"
        multiple
        tabIndex={-1}
        className="sr-only"
        aria-label="Folder to upload"
        onChange={(e) => {
          uploads.upload(entriesFromFiles(e.target.files ?? []));
          e.target.value = '';
        }}
      />
    </>
  );

  let table: ReactNode;
  if (missing) {
    table = (
      <EmptyState
        icon={FolderIcon}
        title="This folder is not here"
        description="It was deleted or moved out of reach."
        action={
          <Button variant="outline" asChild>
            <Link to="/app/artifacts">All files</Link>
          </Button>
        }
      />
    );
  } else if (files.isPending || (!guest && foldersQuery.isPending)) {
    table = <TableSkeleton rows={5} />;
  } else if (files.isError) {
    table = (
      <ErrorState
        title="Couldn’t load the files"
        error={files.error}
        onRetry={() => void files.refetch()}
      />
    );
  } else if (shownFolders.length === 0 && files.data.length === 0) {
    table = filtering ? (
      <EmptyState
        icon={SearchIcon}
        title="No files match"
        description={
          issueId
            ? 'No file is attached to this issue yet.'
            : 'No file or folder has that in its name.'
        }
        action={
          <Button
            variant="outline"
            onClick={() => {
              setQ('');
              setIssue(ANY);
            }}
          >
            Clear the filters
          </Button>
        }
      />
    ) : (
      <EmptyState
        icon={FolderOpenIcon}
        title={folderId || folderParam === ROOT ? 'Nothing here yet' : 'No files yet'}
        description="Keep code, HTML and Markdown with the work: upload files or a whole folder, drop them here, or write a new file."
        action={
          canWrite && (
            <Button onClick={() => filesInput.current?.click()}>
              <UploadIcon />
              Upload files
            </Button>
          )
        }
      />
    );
  } else {
    table = (
      <div className="overflow-hidden rounded-lg border">
        <div
          aria-hidden
          className="flex h-8 items-center gap-3 border-b bg-muted/40 px-3 text-xs text-muted-foreground"
        >
          <span className="flex-1">Name</span>
          <span className="hidden w-24 sm:block">Issue</span>
          <span className="w-12">Version</span>
          <span className="hidden w-16 text-right md:block">Size</span>
          <span className="hidden w-44 md:block">Updated</span>
          <span className="w-7" />
        </div>
        <ul className="divide-y">
          {shownFolders.map((folder) => (
            <FolderRow
              key={folder.id}
              folder={folder}
              drag={
                canWrite
                  ? rows.source(
                      { kind: 'folder', id: folder.id, name: folder.name, from: folder.parent_id },
                      () => folderAndDescendants(folders, folder.id),
                    )
                  : undefined
              }
              drop={rows.target(folder.id)}
              onRename={() => folderActions.rename(folder)}
              onMove={() => folderActions.move(folder)}
              onDelete={() => folderActions.remove(folder)}
            />
          ))}
          {files.data.map((file) => (
            <FileRow
              key={file.id}
              file={file}
              drag={
                canWrite
                  ? rows.source({
                      kind: 'file',
                      id: file.id,
                      name: file.name,
                      from: file.folder_id,
                    })
                  : undefined
              }
              actions={fileActions}
            />
          ))}
        </ul>
      </div>
    );
  }

  return (
    <PageContainer className="relative max-w-none flex-1" {...dropFiles}>
      <PageHeader
        title="Artifacts"
        description="Code, HTML and Markdown kept with the work, every version of it."
        actions={header}
      />
      <div className="flex flex-wrap items-center gap-2">
        <div className="min-w-0 flex-1 basis-60">
          {!guest && <Trail path={path} last={lastCrumb} drop={rows.target} />}
        </div>
        <Input
          value={q}
          placeholder="Search file names…"
          aria-label="Search files"
          onChange={(e) => setQ(e.target.value)}
          className="h-8 w-full text-[13px] sm:w-56"
        />
        <OptionSelect
          value={issueId ?? ANY}
          onValueChange={setIssue}
          options={[
            { value: ANY, label: 'Any issue' },
            ...(issueId && !issue ? [{ value: issueId, label: 'This issue' }] : []),
            ...issues.map((i) => ({ value: i.id, label: `${i.identifier} · ${i.title}` })),
          ]}
          aria-label="Issue"
          className="h-8 w-full text-[13px] sm:w-56"
        />
      </div>
      {uploads.busy && (
        <p role="status" className="flex items-center gap-2 text-[13px] text-muted-foreground">
          <Spinner className="size-3.5" aria-hidden />
          {uploads.sending ? `Uploading ${uploads.sending}…` : 'Uploading…'}
        </p>
      )}
      {table}
      {overPage && (
        <div
          aria-hidden
          className="pointer-events-none absolute inset-2 flex items-center justify-center rounded-xl border-2 border-dashed border-brand/60 bg-background/80 text-sm font-medium"
        >
          Drop to upload into {openFolder?.name ?? (issue ? issue.identifier : 'All files')}
        </div>
      )}
      {asking === 'folder' && (
        <NameDialog
          onClose={() => setAsking(null)}
          title="New folder"
          description={openFolder ? `Inside ${openFolder.name}.` : 'At the top level.'}
          label="Folder name"
          placeholder="design-notes"
          submitLabel="Make folder"
          check={folderNameProblem}
          onSubmit={createFolder}
        />
      )}
      {asking === 'file' && (
        <NameDialog
          onClose={() => setAsking(null)}
          title="New file"
          description="Code, HTML or Markdown: the extension says which. It opens ready to write in."
          label="File name"
          placeholder="notes.md"
          submitLabel="Make file"
          check={fileNameCheck}
          onSubmit={createFile}
        />
      )}
      {fileActions.dialogs}
      {folderActions.dialogs}
    </PageContainer>
  );
}

export default function ArtifactsPage() {
  const { current } = useCurrentWorkspace();
  return (
    <>
      <Seo title="Artifacts" noIndex />
      {current ? <Browser key={current.id} workspace={current} /> : <PageSkeleton />}
    </>
  );
}
