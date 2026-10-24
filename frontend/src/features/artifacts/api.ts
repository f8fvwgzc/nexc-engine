import { queryOptions, type QueryClient } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { saveDownload } from '@/lib/api/download';
import { FRESH } from '@/lib/freshness';
import { qk } from '@/lib/query-keys';
import {
  artifactContentSchema,
  artifactFileSchema,
  artifactFolderSchema,
  artifactVersionSchema,
  runArtifactSchema,
  type ArtifactFile,
} from '@/schemas/artifact';

/** Which files a list holds. With none of these set, every file the caller can see. */
export interface ArtifactFileFilter {
  /** Files directly in this folder. */
  folder_id?: string;
  /** Files in no folder. */
  root?: boolean;
  /** The files of one issue, in any folder. */
  issue_id?: string;
  /** Names containing this, ignoring case. */
  q?: string;
}

/** Every folder of the workspace, flat; `buildFolderTree` nests them. */
export const artifactFoldersQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.artifacts.folders(workspaceId),
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/artifact-folders`, z.array(artifactFolderSchema), {
        signal,
      }),
  });

/** The files that match `filter`, by name (every file: newest change first). */
export const artifactFilesQuery = (workspaceId: string, filter: ArtifactFileFilter = {}) =>
  queryOptions({
    queryKey: qk.artifacts.list(workspaceId, filter),
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/artifact-files`, z.array(artifactFileSchema), {
        query: {
          folder_id: filter.folder_id,
          root: filter.root ? 'true' : undefined,
          issue_id: filter.issue_id,
          q: filter.q,
        },
        signal,
      }),
  });

export const artifactFileQuery = (fileId: string) =>
  queryOptions({
    queryKey: qk.artifacts.file(fileId),
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/artifact-files/${fileId}`, artifactFileSchema, { signal }),
  });

/** A file's saved versions, newest first. */
export const artifactVersionsQuery = (fileId: string) =>
  queryOptions({
    queryKey: qk.artifacts.versions(fileId),
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/artifact-files/${fileId}/versions`, z.array(artifactVersionSchema), {
        signal,
      }),
  });

/** The text of one version. A saved version never changes, so it is never asked for twice. */
export const artifactContentQuery = (fileId: string, version: number) =>
  queryOptions({
    queryKey: qk.artifacts.content(fileId, version),
    staleTime: Infinity,
    queryFn: ({ signal }) =>
      apiRequest(`/artifact-files/${fileId}/content`, artifactContentSchema, {
        query: { version },
        signal,
      }),
  });

export function createArtifactFolder(
  workspaceId: string,
  body: { name: string; parent_id?: string | null },
) {
  return apiRequest(`/workspaces/${workspaceId}/artifact-folders`, artifactFolderSchema, {
    method: 'POST',
    body,
  });
}

/** Renames a folder or moves it under another one (`parent_id: null` is the top level). */
export function updateArtifactFolder(
  workspaceId: string,
  folderId: string,
  body: { name?: string; parent_id?: string | null },
) {
  return apiRequest(
    `/workspaces/${workspaceId}/artifact-folders/${folderId}`,
    artifactFolderSchema,
    { method: 'PATCH', body },
  );
}

/** Deletes a folder with its subfolders and their files. */
export function deleteArtifactFolder(workspaceId: string, folderId: string) {
  return apiSend(`/workspaces/${workspaceId}/artifact-folders/${folderId}`, { method: 'DELETE' });
}

export interface NewArtifactFile {
  name: string;
  folder_id?: string | null;
  issue_id?: string | null;
}

/** Sends a file's bytes as they are; an empty body makes a new, empty file. */
export function uploadArtifactFile(workspaceId: string, file: NewArtifactFile, body: Blob) {
  return apiRequest(`/workspaces/${workspaceId}/artifact-files`, artifactFileSchema, {
    method: 'POST',
    query: { name: file.name, folder_id: file.folder_id, issue_id: file.issue_id },
    body,
  });
}

/**
 * Saves new content over `baseVersion`. A 409 means someone else saved a newer version first;
 * content equal to the current version saves nothing and answers the file as it is.
 */
export function saveArtifactContent(fileId: string, baseVersion: number, content: Blob | string) {
  const body = typeof content === 'string' ? new Blob([content], { type: 'text/plain' }) : content;
  return apiRequest(`/artifact-files/${fileId}/content`, artifactFileSchema, {
    method: 'PUT',
    query: { base_version: baseVersion },
    body,
  });
}

/** Renames a file, moves it to another folder, or attaches it to another issue. */
export function updateArtifactFile(
  fileId: string,
  body: { name?: string; folder_id?: string | null; issue_id?: string | null },
) {
  return apiRequest(`/artifact-files/${fileId}`, artifactFileSchema, { method: 'PATCH', body });
}

/** Copies the current version into a new file (" copy" is added to its name by default). */
export function duplicateArtifactFile(
  fileId: string,
  body: { folder_id?: string | null; name?: string } = {},
) {
  return apiRequest(`/artifact-files/${fileId}/duplicate`, artifactFileSchema, {
    method: 'POST',
    body,
  });
}

export function deleteArtifactFile(fileId: string) {
  return apiSend(`/artifact-files/${fileId}`, { method: 'DELETE' });
}

/** Downloads a version of a file (the current one by default) with the caller's token. */
export function downloadArtifactFile(file: ArtifactFile, version?: number) {
  const query = version === undefined ? '' : `?version=${version}`;
  return saveDownload(`/artifact-files/${file.id}/raw${query}`, file.name);
}

/** The files the runs of an issue's graph produced, newest run first. */
export const issueRunArtifactsQuery = (issueId: string) =>
  queryOptions({
    queryKey: qk.artifacts.runs(issueId),
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/issues/${issueId}/run-artifacts`, z.array(runArtifactSchema), { signal }),
  });

/** Copies a text file a run produced into the issue's artifacts. */
export function keepRunArtifact(
  issueId: string,
  artifactId: string,
  body: { folder_id?: string | null } = {},
) {
  return apiRequest(`/issues/${issueId}/run-artifacts/${artifactId}/keep`, artifactFileSchema, {
    method: 'POST',
    body,
  });
}

/**
 * After a change to files or folders: the lists and the tree are asked for again, and so is the
 * timeline of the issue the change was recorded on.
 */
export function invalidateArtifacts(
  queryClient: QueryClient,
  workspaceId: string,
  issueId?: string | null,
) {
  void queryClient.invalidateQueries({ queryKey: qk.artifacts.lists(workspaceId) });
  void queryClient.invalidateQueries({ queryKey: qk.artifacts.folders(workspaceId) });
  if (issueId) void queryClient.invalidateQueries({ queryKey: qk.issues.events(issueId) });
}
