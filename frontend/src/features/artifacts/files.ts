import type { ArtifactFile, ArtifactFolder } from '@/schemas/artifact';

/** The most one version of a file may hold: 1 MiB, the API's limit. */
export const MAX_ARTIFACT_BYTES = 1_048_576;

/** Said for anything the API would refuse as not text, before it is sent. */
export const NOT_TEXT_MESSAGE = 'Only text files — code, HTML, Markdown — can be artifacts';

/** The extensions the API accepts, ignoring case (CONTRACT: artifacts). */
const EXTENSIONS = new Set(
  (
    'md markdown mdx txt html htm css scss sass less js jsx mjs cjs ts tsx mts cts json jsonc ' +
    'json5 yaml yml toml ini cfg conf env.example xml svg csv tsv sql graphql gql sh bash zsh ' +
    'fish ps1 bat py rb go rs java kt kts scala swift c h cc cpp cxx hpp hh cs fs php pl lua r ' +
    'dart ex exs erl hs clj elm vue svelte astro proto tf hcl dockerfile makefile gitignore ' +
    'editorconfig lock log diff patch rst adoc tex'
  ).split(' '),
);

/** Names accepted whole, without an extension of their own, in lower case: any case matches. */
const BASENAMES = new Set([
  'dockerfile',
  'makefile',
  '.gitignore',
  '.editorconfig',
  '.env.example',
]);

const ENV_EXAMPLE = '.env.example';

/** The part of a path after its last slash. */
export function baseName(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1);
}

/**
 * A name's extension in lower case, without the dot: what follows the last dot, unless that dot
 * starts the name (`.gitignore` has none). `x.env.example` is the one two-part extension.
 */
export function artifactExtension(name: string): string {
  const lower = name.toLowerCase();
  if (lower.endsWith(ENV_EXAMPLE) && lower.length > ENV_EXAMPLE.length) return 'env.example';
  const dot = lower.lastIndexOf('.');
  return dot > 0 ? lower.slice(dot + 1) : '';
}

/** Whether a file of this name can be an artifact: code, markup, config or text. */
export function isArtifactName(name: string): boolean {
  return BASENAMES.has(name.toLowerCase()) || EXTENSIONS.has(artifactExtension(name));
}

/** Characters as the API counts them (code points, not UTF-16 units). */
const length = (text: string) => [...text].length;

function nameProblem(name: string, max: number, what: string): string | null {
  if (length(name) === 0) return `Give the ${what} a name.`;
  if (length(name) > max) return `A ${what} name is at most ${max} characters.`;
  if (/[/\\]/.test(name)) return `A ${what} name cannot hold a slash.`;
  if (/\p{Cc}/u.test(name)) return `A ${what} name cannot hold control characters.`;
  return null;
}

/** Why a file cannot have this name, or null: 1–200 characters, no slashes or control characters. */
export function fileNameProblem(name: string): string | null {
  return nameProblem(name, 200, 'file');
}

/** Why a folder cannot have this name, or null: 1–120 characters, no slashes or control characters. */
export function folderNameProblem(name: string): string | null {
  return nameProblem(name, 120, 'folder');
}

/** Why a file cannot be uploaded as an artifact, or null: its name, its kind and its size. */
export function artifactFileProblem(file: { name: string; size: number }): string | null {
  const named = fileNameProblem(file.name);
  if (named) return named;
  if (!isArtifactName(file.name)) return NOT_TEXT_MESSAGE;
  if (file.size > MAX_ARTIFACT_BYTES) return 'Larger than 1 MB, the most a file may hold';
  return null;
}

/** Why a file cannot be named this, or null: the name rules, and an extension the API takes. */
export const fileNameCheck = (name: string) => artifactFileProblem({ name, size: 0 });

/** Whether a file has a rendered preview besides its source: Markdown and HTML. */
export const hasPreview = (file: Pick<ArtifactFile, 'kind'>) =>
  file.kind === 'markdown' || file.kind === 'html';

const byName = (a: { name: string }, b: { name: string }) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: 'base', numeric: true });

/** Sorts files or folders by name the way people read them: ignoring case, `2` before `10`. */
export function sortByName<T extends { name: string }>(items: readonly T[]): T[] {
  return [...items].sort(byName);
}

export interface FolderNode {
  folder: ArtifactFolder;
  children: FolderNode[];
}

/**
 * Nests the flat folder list under each folder's parent, every level by name. A folder whose
 * parent is not in the list (one the caller cannot see) sits at the top.
 */
export function buildFolderTree(folders: readonly ArtifactFolder[]): FolderNode[] {
  const ids = new Set(folders.map((f) => f.id));
  const byParent = new Map<string | null, ArtifactFolder[]>();
  for (const folder of folders) {
    const parent =
      folder.parent_id && folder.parent_id !== folder.id && ids.has(folder.parent_id)
        ? folder.parent_id
        : null;
    byParent.set(parent, [...(byParent.get(parent) ?? []), folder]);
  }
  // Each folder is placed once, so a loop in the data cannot nest forever.
  const placed = new Set<string>();
  const grow = (parent: string | null): FolderNode[] =>
    sortByName(byParent.get(parent) ?? [])
      .filter((folder) => !placed.has(folder.id))
      .map((folder) => {
        placed.add(folder.id);
        return { folder, children: grow(folder.id) };
      });
  return grow(null);
}

/** Every folder of a tree in reading order, with how deep it sits (0 at the top). */
export function flattenTree(
  tree: readonly FolderNode[],
  depth = 0,
): { folder: ArtifactFolder; depth: number }[] {
  return tree.flatMap((node) => [
    { folder: node.folder, depth },
    ...flattenTree(node.children, depth + 1),
  ]);
}

/** The folders from the top down to `folderId`, itself included; empty when it is unknown. */
export function folderPath(folders: readonly ArtifactFolder[], folderId: string): ArtifactFolder[] {
  const byId = new Map(folders.map((f) => [f.id, f]));
  const path: ArtifactFolder[] = [];
  let current = byId.get(folderId);
  while (current && !path.includes(current)) {
    path.unshift(current);
    current = current.parent_id ? byId.get(current.parent_id) : undefined;
  }
  return path;
}

/** `folderId` and every folder under it: where a folder cannot be moved to. */
export function folderAndDescendants(
  folders: readonly ArtifactFolder[],
  folderId: string,
): Set<string> {
  const found = new Set([folderId]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const folder of folders) {
      if (folder.parent_id && found.has(folder.parent_id) && !found.has(folder.id)) {
        found.add(folder.id);
        grew = true;
      }
    }
  }
  return found;
}

/** The folder of this name directly under `parentId` (null: the top level), ignoring case. */
export function childFolder(
  folders: readonly ArtifactFolder[],
  parentId: string | null,
  name: string,
): ArtifactFolder | undefined {
  const lower = name.toLowerCase();
  return folders.find((f) => f.parent_id === parentId && f.name.toLowerCase() === lower);
}

/** A folder path as one key: folder names are unique per parent ignoring case. */
export const folderKey = (dirs: readonly string[]) => dirs.join('/').toLowerCase();

export interface UploadPlan<F> {
  /** The folders the files go into, as paths of names, every parent before its children. */
  folders: string[][];
  /** The files to send, each with the folder path it goes into (empty: the target folder). */
  files: { file: F; dirs: string[] }[];
  /** What is not sent, and why. */
  skipped: { path: string; reason: string }[];
}

/**
 * How to recreate a picked or dropped folder under the open one: the folders each file's
 * relative path (`webkitRelativePath`, like `site/src/app.ts`) needs, and the files, minus those
 * the API would refuse. A folder that would hold only refused files is not made.
 */
export function planFolderUpload<F extends { name: string; size: number }>(
  entries: readonly { file: F; path: string }[],
): UploadPlan<F> {
  const plan: UploadPlan<F> = { folders: [], files: [], skipped: [] };
  // `Src` and `src` are one folder, spelled as it was first seen.
  const known = new Map<string, string[]>();
  for (const { file, path } of entries) {
    const parts = (path || file.name).split('/').filter(Boolean);
    const problem = artifactFileProblem(file);
    if (problem) {
      plan.skipped.push({ path: parts.join('/') || file.name, reason: problem });
      continue;
    }
    const dirs = parts.slice(0, -1);
    const badDir = dirs.map(folderNameProblem).find(Boolean);
    if (badDir) {
      plan.skipped.push({ path: parts.join('/'), reason: badDir });
      continue;
    }
    for (let depth = 1; depth <= dirs.length; depth += 1) {
      const key = folderKey(dirs.slice(0, depth));
      if (!known.has(key)) {
        const parent = known.get(folderKey(dirs.slice(0, depth - 1))) ?? [];
        const folder = [...parent, dirs[depth - 1]!];
        known.set(key, folder);
        plan.folders.push(folder);
      }
    }
    plan.files.push({ file, dirs });
  }
  plan.folders.sort((a, b) => a.length - b.length);
  return plan;
}
