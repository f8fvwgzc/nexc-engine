import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { ArtifactFile, ArtifactFolder } from '@/schemas/artifact';

import {
  artifactFoldersQuery,
  deleteArtifactFile,
  deleteArtifactFolder,
  downloadArtifactFile,
  duplicateArtifactFile,
  invalidateArtifacts,
  updateArtifactFile,
  updateArtifactFolder,
} from './api';
import { MoveDialog, NameDialog } from './artifact-dialogs';
import { fileNameCheck, folderAndDescendants, folderNameProblem } from './files';

type FileAsk = { kind: 'rename' | 'move' | 'delete'; file: ArtifactFile } | null;

/**
 * What can be done to a file (rename, move, duplicate, download, delete), with the dialogs that
 * ask first. Render `dialogs` once wherever the actions are offered.
 */
export function useFileActions(
  workspaceId: string,
  {
    onDeleted,
    onDuplicated,
  }: {
    onDeleted?: (file: ArtifactFile) => void;
    /** After a copy is made (it is announced in a toast either way). */
    onDuplicated?: (copy: ArtifactFile) => void;
  } = {},
) {
  const queryClient = useQueryClient();
  const [ask, setAsk] = useState<FileAsk>(null);
  const close = () => setAsk(null);
  const changed = (file: ArtifactFile) => {
    queryClient.setQueryData(qk.artifacts.file(file.id), file);
    invalidateArtifacts(queryClient, workspaceId, file.issue?.id);
  };
  const duplicate = useMutation({
    mutationFn: (file: ArtifactFile) => duplicateArtifactFile(file.id),
    onSuccess: (copy) => {
      toast.success(`Duplicated as ${copy.name}`);
      invalidateArtifacts(queryClient, workspaceId, copy.issue?.id);
      onDuplicated?.(copy);
    },
  });
  const remove = useMutation({
    mutationFn: (file: ArtifactFile) => deleteArtifactFile(file.id),
    meta: { successMessage: 'File deleted' },
    onSuccess: (_, file) => {
      queryClient.removeQueries({ queryKey: qk.artifacts.file(file.id) });
      invalidateArtifacts(queryClient, workspaceId, file.issue?.id);
      onDeleted?.(file);
    },
  });
  const download = (file: ArtifactFile, version?: number) => {
    downloadArtifactFile(file, version).catch((error: unknown) => toast.error(errorMessage(error)));
  };

  const dialogs = (
    <>
      {ask?.kind === 'rename' && (
        <NameDialog
          onClose={close}
          title="Rename file"
          label="File name"
          initial={ask.file.name}
          submitLabel="Rename"
          selectStem
          check={fileNameCheck}
          onSubmit={async (name) => changed(await updateArtifactFile(ask.file.id, { name }))}
        />
      )}
      {ask?.kind === 'move' && (
        <MoveDialog
          workspaceId={workspaceId}
          onClose={close}
          title={`Move ${ask.file.name}`}
          current={ask.file.folder_id}
          topLabel="Not in a folder"
          onMove={async (folderId) =>
            changed(await updateArtifactFile(ask.file.id, { folder_id: folderId }))
          }
        />
      )}
      {ask?.kind === 'delete' && (
        <ConfirmDialog
          open
          onOpenChange={(open) => !open && close()}
          title={`Delete ${ask.file.name}?`}
          description="The file and every version of it are deleted. This cannot be undone."
          confirmLabel="Delete file"
          destructive
          onConfirm={() => remove.mutate(ask.file)}
        />
      )}
    </>
  );

  return {
    rename: (file: ArtifactFile) => setAsk({ kind: 'rename', file }),
    move: (file: ArtifactFile) => setAsk({ kind: 'move', file }),
    remove: (file: ArtifactFile) => setAsk({ kind: 'delete', file }),
    duplicate: (file: ArtifactFile) => duplicate.mutate(file),
    download,
    dialogs,
  };
}

type FolderAsk = { kind: 'rename' | 'move' | 'delete'; folder: ArtifactFolder } | null;

/** What can be done to a folder (rename, move, delete with what it holds), with its dialogs. */
export function useFolderActions(
  workspaceId: string,
  { onDeleted }: { onDeleted?: (folder: ArtifactFolder) => void } = {},
) {
  const queryClient = useQueryClient();
  const [ask, setAsk] = useState<FolderAsk>(null);
  const { data: folders = [] } = useQuery({
    ...artifactFoldersQuery(workspaceId),
    enabled: ask !== null,
  });
  const close = () => setAsk(null);
  const changed = () => invalidateArtifacts(queryClient, workspaceId);
  const remove = useMutation({
    mutationFn: (folder: ArtifactFolder) => deleteArtifactFolder(workspaceId, folder.id),
    meta: { successMessage: 'Folder deleted' },
    onSuccess: (_, folder) => {
      changed();
      onDeleted?.(folder);
    },
  });

  const dialogs = (
    <>
      {ask?.kind === 'rename' && (
        <NameDialog
          onClose={close}
          title="Rename folder"
          label="Folder name"
          initial={ask.folder.name}
          submitLabel="Rename"
          check={folderNameProblem}
          onSubmit={async (name) => {
            await updateArtifactFolder(workspaceId, ask.folder.id, { name });
            changed();
          }}
        />
      )}
      {ask?.kind === 'move' && (
        <MoveDialog
          workspaceId={workspaceId}
          onClose={close}
          title={`Move ${ask.folder.name}`}
          current={ask.folder.parent_id}
          topLabel="Top level"
          exclude={folderAndDescendants(folders, ask.folder.id)}
          onMove={async (parentId) => {
            await updateArtifactFolder(workspaceId, ask.folder.id, { parent_id: parentId });
            changed();
          }}
        />
      )}
      {ask?.kind === 'delete' && (
        <ConfirmDialog
          open
          onOpenChange={(open) => !open && close()}
          title={`Delete ${ask.folder.name}?`}
          description="Its subfolders and every file in them are deleted too. This cannot be undone."
          confirmLabel="Delete folder"
          destructive
          onConfirm={() => remove.mutate(ask.folder)}
        />
      )}
    </>
  );

  return {
    rename: (folder: ArtifactFolder) => setAsk({ kind: 'rename', folder }),
    move: (folder: ArtifactFolder) => setAsk({ kind: 'move', folder }),
    remove: (folder: ArtifactFolder) => setAsk({ kind: 'delete', folder }),
    dialogs,
  };
}
