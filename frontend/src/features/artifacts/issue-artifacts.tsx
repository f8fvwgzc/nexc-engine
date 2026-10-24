import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CopyIcon,
  DownloadIcon,
  FileIcon,
  LockIcon,
  MoreHorizontalIcon,
  PlusIcon,
  RotateCcwIcon,
  Trash2Icon,
  UploadIcon,
} from 'lucide-react';
import { useRef, useState, type ReactNode } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { toast } from 'sonner';

import { IconButton } from '@/components/custom-ui/icon-button';
import { StatusBadge } from '@/components/custom-ui/status-badge';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Skeleton } from '@/components/ui/skeleton';
import { downloadArtifact } from '@/features/runs/api';
import { errorMessage } from '@/lib/api/errors';
import { formatBytes, formatDateTime, formatRelative } from '@/lib/format';
import { isDownloadable } from '@/schemas/run';
import type { RunArtifact } from '@/schemas/artifact';
import type { Issue } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import {
  artifactFilesQuery,
  invalidateArtifacts,
  issueRunArtifactsQuery,
  keepRunArtifact,
  uploadArtifactFile,
} from './api';
import { NameDialog } from './artifact-dialogs';
import { ArtifactGlyph } from './artifact-glyphs';
import { baseName, fileNameCheck, isArtifactName, MAX_ARTIFACT_BYTES } from './files';
import { useFileActions } from './use-artifact-actions';
import { entriesFromFiles, useArtifactUpload } from './use-artifact-upload';

/** Whether a run's file can be kept as an artifact: downloadable, text by name, small enough. */
const keepable = (artifact: RunArtifact) =>
  isDownloadable(artifact) &&
  isArtifactName(baseName(artifact.path)) &&
  artifact.size <= MAX_ARTIFACT_BYTES;

/** The files the issue's runs produced, each a download, and the text ones kept with a click. */
function FromRuns({ workspace, issue }: { workspace: Workspace; issue: Issue }) {
  const queryClient = useQueryClient();
  const { data: produced = [] } = useQuery({
    ...issueRunArtifactsQuery(issue.id),
    enabled: issue.graph_id !== null,
  });
  const keep = useMutation({
    mutationFn: (artifact: RunArtifact) => keepRunArtifact(issue.id, artifact.id),
    onSuccess: (file) => {
      toast.success(`Kept ${file.name} with the issue`);
      invalidateArtifacts(queryClient, workspace.id, issue.id);
    },
  });
  if (produced.length === 0) return null;
  const canWrite = workspace.role !== 'guest';
  return (
    <div className="space-y-1.5">
      <h4 className="text-xs font-medium text-muted-foreground">From runs</h4>
      <ul className="divide-y rounded-md border">
        {produced.map((artifact) => {
          const held = !isDownloadable(artifact);
          return (
            <li key={artifact.id} className="flex h-8 items-center gap-2.5 px-3 text-[13px]">
              {held ? (
                <LockIcon className="size-3.5 shrink-0 text-destructive" aria-label="Held back" />
              ) : (
                <FileIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
              )}
              <span
                className="min-w-0 flex-1 truncate font-mono text-xs"
                title={held ? `Held back: ${artifact.quarantine_reason}` : artifact.path}
              >
                {artifact.path}
              </span>
              <StatusBadge status={artifact.run_status} className="h-5 px-1.5" />
              <span className="hidden w-16 text-right text-xs text-muted-foreground tabular-nums sm:block">
                {formatBytes(artifact.size)}
              </span>
              {canWrite && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 px-2 text-xs"
                  aria-label={`Keep ${baseName(artifact.path)} with the issue`}
                  title={
                    keepable(artifact)
                      ? 'Copy into the issue’s artifacts'
                      : 'Only text files up to 1 MB can be kept'
                  }
                  disabled={!keepable(artifact) || keep.isPending}
                  onClick={() => keep.mutate(artifact)}
                >
                  Keep
                </Button>
              )}
              <IconButton
                label={`Download ${artifact.path}`}
                size="icon-xs"
                disabled={held}
                onClick={() =>
                  void downloadArtifact(artifact).catch((error: unknown) =>
                    toast.error(errorMessage(error)),
                  )
                }
              >
                <DownloadIcon />
              </IconButton>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/**
 * The issue's artifacts: the text files attached to it (each opens on its own page) with
 * Upload, New file, duplicate and delete, and the files its runs produced, which can be kept as
 * artifacts. The heading, Upload and New file are there while the files load and when they could
 * not be read. A name the issue already has is refused: the server's word shows in the New file
 * dialog, or in the upload's toast.
 */
export function IssueArtifacts({ workspace, issue }: { workspace: Workspace; issue: Issue }) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const canWrite = workspace.role !== 'guest';
  const files = useQuery(artifactFilesQuery(workspace.id, { issue_id: issue.id }));
  const actions = useFileActions(workspace.id);
  const uploads = useArtifactUpload(workspace.id, { folderId: null, issueId: issue.id });
  const [naming, setNaming] = useState(false);
  const picker = useRef<HTMLInputElement>(null);

  const createFile = async (name: string) => {
    const file = await uploadArtifactFile(workspace.id, { name, issue_id: issue.id }, new Blob([]));
    invalidateArtifacts(queryClient, workspace.id, issue.id);
    void navigate(`/app/artifacts/${file.id}?edit=1`);
  };

  // A list that could not be read is not an empty one. A failed refetch keeps the list it had.
  let list: ReactNode;
  if (files.isPending) {
    list = (
      <div role="status" aria-busy aria-label="Loading the files">
        <Skeleton className="h-8 w-full" />
      </div>
    );
  } else if (files.data === undefined) {
    list = (
      <div role="alert" className="flex items-center gap-2 text-sm text-muted-foreground">
        <span className="flex-1">Couldn’t load the files.</span>
        <Button variant="ghost" size="sm" onClick={() => void files.refetch()}>
          <RotateCcwIcon />
          Try again
        </Button>
      </div>
    );
  } else if (files.data.length > 0) {
    list = (
      <ul className="divide-y rounded-md border">
        {files.data.map((file) => (
          <li
            key={file.id}
            className="relative flex h-8 items-center gap-2.5 px-3 text-[13px] hover:bg-muted/60"
          >
            <ArtifactGlyph kind={file.kind} />
            <Link
              to={`/app/artifacts/${file.id}`}
              title={file.name}
              className="min-w-0 flex-1 truncate outline-none after:absolute after:inset-0 focus-visible:underline"
            >
              {file.name}
            </Link>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
              v{file.current_version}
            </span>
            <span
              className="hidden w-24 shrink-0 truncate text-right text-xs text-muted-foreground sm:block"
              title={formatDateTime(file.updated_at)}
            >
              {formatRelative(file.updated_at)}
            </span>
            <span className="relative z-10">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <IconButton label={`Actions for ${file.name}`} size="icon-xs">
                    <MoreHorizontalIcon />
                  </IconButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  {canWrite && (
                    <DropdownMenuItem onSelect={() => actions.duplicate(file)}>
                      <CopyIcon />
                      Duplicate
                    </DropdownMenuItem>
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
        ))}
      </ul>
    );
  } else {
    list = (
      <p className="text-xs text-muted-foreground">
        No files yet. Code, HTML and Markdown attached here keep every version.
      </p>
    );
  }

  return (
    <section aria-label="Artifacts" className="space-y-2 border-t pt-4">
      <div className="flex items-center gap-2">
        <h3 className="text-[13px] font-medium text-muted-foreground">Artifacts</h3>
        {files.data && files.data.length > 0 && (
          <span className="text-xs text-muted-foreground tabular-nums">{files.data.length}</span>
        )}
        {canWrite && (
          <div className="ml-auto flex items-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              className="h-6 gap-1.5 px-2 text-xs"
              onClick={() => setNaming(true)}
            >
              <PlusIcon className="size-3.5" />
              New file
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-6 gap-1.5 px-2 text-xs"
              disabled={uploads.busy}
              onClick={() => picker.current?.click()}
            >
              <UploadIcon className="size-3.5" />
              {uploads.busy ? 'Uploading…' : 'Upload'}
            </Button>
            <input
              ref={picker}
              type="file"
              multiple
              tabIndex={-1}
              className="sr-only"
              aria-label={`Files to attach to ${issue.identifier}`}
              onChange={(e) => {
                uploads.upload(entriesFromFiles(e.target.files ?? []));
                e.target.value = '';
              }}
            />
          </div>
        )}
      </div>
      {list}
      <FromRuns workspace={workspace} issue={issue} />
      {naming && (
        <NameDialog
          onClose={() => setNaming(false)}
          title="New file"
          description={`Attached to ${issue.identifier}. It opens ready to write in.`}
          label="File name"
          placeholder="notes.md"
          submitLabel="Make file"
          check={fileNameCheck}
          onSubmit={createFile}
        />
      )}
      {actions.dialogs}
    </section>
  );
}
