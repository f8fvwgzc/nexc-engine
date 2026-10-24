import { useQueryClient } from '@tanstack/react-query';
import { useRef, useState } from 'react';
import { toast } from 'sonner';

import { errorMessage } from '@/lib/api/errors';
import type { ArtifactFolder } from '@/schemas/artifact';

import {
  artifactFoldersQuery,
  createArtifactFolder,
  invalidateArtifacts,
  uploadArtifactFile,
} from './api';
import { childFolder, folderKey, planFolderUpload } from './files';

/** A file to send, with where it sits in what was picked or dropped (`site/src/app.ts`). */
export interface UploadEntry {
  file: File;
  path: string;
}

/** Picked files, keeping the folder path a folder picker gives each one. */
export const entriesFromFiles = (files: Iterable<File>): UploadEntry[] =>
  [...files].map((file) => ({ file, path: file.webkitRelativePath || file.name }));

function readDirectory(dir: FileSystemDirectoryEntry): Promise<FileSystemEntry[]> {
  const reader = dir.createReader();
  const all: FileSystemEntry[] = [];
  // A reader hands entries over in batches until it hands over none.
  return new Promise((resolve, reject) => {
    const next = () =>
      reader.readEntries((batch) => {
        if (batch.length === 0) return resolve(all);
        all.push(...batch);
        next();
      }, reject);
    next();
  });
}

async function walk(entry: FileSystemEntry): Promise<UploadEntry[]> {
  if (entry.isFile) {
    const file = await new Promise<File>((resolve, reject) =>
      (entry as FileSystemFileEntry).file(resolve, reject),
    );
    return [{ file, path: entry.fullPath }];
  }
  if (entry.isDirectory) {
    const children = await readDirectory(entry as FileSystemDirectoryEntry);
    return (await Promise.all(children.map(walk))).flat();
  }
  return [];
}

/**
 * What was dropped from the desktop, folders included with their structure. The entries are
 * taken from the event at once: the browser empties `dataTransfer` once the handler returns.
 */
export function entriesFromDrop(data: DataTransfer): Promise<UploadEntry[]> {
  const roots = [...data.items]
    .filter((item) => item.kind === 'file')
    .map((item) => item.webkitGetAsEntry())
    .filter((entry): entry is FileSystemEntry => entry !== null);
  if (roots.length === 0) return Promise.resolve(entriesFromFiles(data.files));
  return Promise.all(roots.map(walk)).then((lists) => lists.flat());
}

/** Whether a drag carries files from the desktop (not a row of the page). */
export const dragHasFiles = (data: DataTransfer) => data.types.includes('Files');

function summarise(sent: number, skipped: { path: string; reason: string }[]) {
  const files = (n: number) => `${n} file${n === 1 ? '' : 's'}`;
  if (skipped.length === 0) {
    if (sent > 0) toast.success(`Uploaded ${files(sent)}`);
    return;
  }
  const description = (
    <ul className="space-y-0.5">
      {skipped.slice(0, 5).map((s) => (
        <li key={s.path} className="break-words">
          {s.path}: {s.reason}
        </li>
      ))}
      {skipped.length > 5 && <li>and {skipped.length - 5} more</li>}
    </ul>
  );
  const title =
    sent > 0
      ? `Uploaded ${files(sent)}; skipped ${skipped.length}`
      : `Nothing uploaded: ${files(skipped.length)} skipped`;
  if (sent > 0) toast.warning(title, { description });
  else toast.error(title, { description });
}

/**
 * Sends files into a folder (null: none) and, for an issue, attaches them to it. A picked or
 * dropped folder is recreated under that folder, reusing folders of the same name; files the API
 * would refuse are skipped and listed in one toast at the end. An upload started while another
 * is still sending waits for it, so the two neither mix their status nor race to make a folder.
 */
export function useArtifactUpload(
  workspaceId: string,
  target: { folderId: string | null; issueId?: string | null },
) {
  const queryClient = useQueryClient();
  const [sending, setSending] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const waiting = useRef(0);

  const run = async (entries: UploadEntry[]) => {
    const plan = planFolderUpload(entries);
    const skipped = [...plan.skipped];
    const ids = new Map<string, string | null>([['', target.folderId]]);
    if (plan.folders.length > 0) {
      // The tree as it is now, so an existing folder is reused rather than refused as taken.
      let folders: ArtifactFolder[] = await queryClient
        .fetchQuery({ ...artifactFoldersQuery(workspaceId), staleTime: 0 })
        .catch(() => []);
      for (const dirs of plan.folders) {
        const parent = ids.get(folderKey(dirs.slice(0, -1)));
        if (parent === undefined) continue;
        const name = dirs[dirs.length - 1] ?? '';
        const existing = childFolder(folders, parent, name);
        if (existing) {
          ids.set(folderKey(dirs), existing.id);
          continue;
        }
        setSending(`${dirs.join('/')}/`);
        try {
          const made = await createArtifactFolder(workspaceId, { name, parent_id: parent });
          folders = [...folders, made];
          ids.set(folderKey(dirs), made.id);
        } catch (error) {
          skipped.push({ path: `${dirs.join('/')}/`, reason: errorMessage(error) });
        }
      }
    }
    let sent = 0;
    for (const { file, dirs } of plan.files) {
      const path = [...dirs, file.name].join('/');
      const folderId = ids.get(folderKey(dirs));
      if (folderId === undefined) {
        skipped.push({ path, reason: 'its folder could not be made' });
        continue;
      }
      setSending(path);
      try {
        await uploadArtifactFile(
          workspaceId,
          { name: file.name, folder_id: folderId, issue_id: target.issueId },
          file,
        );
        sent += 1;
      } catch (error) {
        skipped.push({ path, reason: errorMessage(error) });
      }
    }
    invalidateArtifacts(queryClient, workspaceId, target.issueId);
    summarise(sent, skipped);
  };

  const upload = (entries: UploadEntry[]) => {
    if (entries.length === 0) return;
    waiting.current += 1;
    setBusy(true);
    queue.current = queue.current
      .then(() => run(entries))
      .catch((error: unknown) => {
        toast.error(errorMessage(error));
      })
      .finally(() => {
        waiting.current -= 1;
        if (waiting.current === 0) {
          setBusy(false);
          setSending(null);
        }
      });
  };

  return {
    /** What is being sent right now, or null. */
    sending,
    /** Whether an upload is under way or waiting its turn. */
    busy,
    upload,
  };
}
