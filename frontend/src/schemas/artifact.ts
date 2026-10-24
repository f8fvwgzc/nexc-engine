import { z } from 'zod';

import { idSchema, timestampSchema } from './common';
import { artifactSchema, runStatusSchema } from './run';

/** Who made or last changed something, while they are still a member. */
const personSchema = z.object({ user_id: idSchema, name: z.string() }).nullable();

/** A folder of the workspace's artifacts. The list is flat; the client builds the tree. */
export const artifactFolderSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  parent_id: idSchema.nullable(),
  name: z.string(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type ArtifactFolder = z.infer<typeof artifactFolderSchema>;

/** How the UI previews a file: rendered Markdown, a sandboxed page, or highlighted source. */
export const artifactKindSchema = z.enum(['markdown', 'html', 'code', 'text']);
export type ArtifactKind = z.infer<typeof artifactKindSchema>;

/** A text file people and agents keep with the work: code, HTML or Markdown, with versions. */
export const artifactFileSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  folder_id: idSchema.nullable(),
  name: z.string(),
  /** Lower-case, without the dot; empty for names like `Dockerfile`. */
  extension: z.string(),
  kind: artifactKindSchema,
  /** The highlighter's language id (`typescript`, `rust`, `plaintext`…). */
  language: z.string(),
  /** The issue the file belongs to, if any. */
  issue: z.object({ id: idSchema, identifier: z.string(), title: z.string() }).nullable(),
  current_version: z.number().int().nonnegative(),
  size: z.number().int().nonnegative(),
  sha256: z.string(),
  /** The file this one was copied from. */
  duplicated_from: idSchema.nullable(),
  created_by: personSchema,
  updated_by: personSchema,
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type ArtifactFile = z.infer<typeof artifactFileSchema>;

/** One saved version of a file, newest first in a list. */
export const artifactVersionSchema = z.object({
  version: z.number().int().nonnegative(),
  size: z.number().int().nonnegative(),
  sha256: z.string(),
  created_by: personSchema,
  created_at: timestampSchema,
});
export type ArtifactVersion = z.infer<typeof artifactVersionSchema>;

/** The text of one version of a file. */
export const artifactContentSchema = z.object({
  file_id: idSchema,
  version: z.number().int().nonnegative(),
  content: z.string(),
});
export type ArtifactContent = z.infer<typeof artifactContentSchema>;

/** A file a run of the issue's graph produced, with where that run stands. */
export const runArtifactSchema = artifactSchema.extend({
  run_status: runStatusSchema,
  run_created_at: timestampSchema,
});
export type RunArtifact = z.infer<typeof runArtifactSchema>;
