import { describe, expect, it } from 'vitest';

import type { ArtifactFolder } from '@/schemas/artifact';

import {
  MAX_ARTIFACT_BYTES,
  NOT_TEXT_MESSAGE,
  artifactExtension,
  artifactFileProblem,
  buildFolderTree,
  childFolder,
  fileNameProblem,
  flattenTree,
  folderAndDescendants,
  folderNameProblem,
  folderPath,
  isArtifactName,
  planFolderUpload,
} from './files';

const at = '2026-10-24T09:00:00Z';
const ws = '01900000-0000-7000-8000-0000000000aa';
let next = 0;

function folder(name: string, parent: ArtifactFolder | null = null): ArtifactFolder {
  next += 1;
  return {
    id: `01900000-0000-7000-8000-${String(next).padStart(12, '0')}`,
    workspace_id: ws,
    parent_id: parent?.id ?? null,
    name,
    created_at: at,
    updated_at: at,
  };
}

describe('the extension allow-list', () => {
  it('takes code, markup, config and text in any case', () => {
    for (const name of ['notes.md', 'App.TSX', 'main.rs', 'index.html', 'style.SCSS', 'q.sql']) {
      expect(isArtifactName(name), name).toBe(true);
    }
  });

  it('refuses documents, images and binaries', () => {
    for (const name of ['report.pdf', 'brief.docx', 'logo.png', 'app.exe', 'data.xlsx']) {
      expect(isArtifactName(name), name).toBe(false);
    }
  });

  it('knows the names that are their own kind, exactly as written', () => {
    expect(isArtifactName('Dockerfile')).toBe(true);
    expect(isArtifactName('Makefile')).toBe(true);
    expect(isArtifactName('.gitignore')).toBe(true);
    expect(isArtifactName('.editorconfig')).toBe(true);
    expect(isArtifactName('.env.example')).toBe(true);
    expect(isArtifactName('README')).toBe(false);
    expect(isArtifactName('.bashrc')).toBe(false);
    expect(isArtifactName('.env')).toBe(false);
  });

  it('reads the extension after the last dot, but not a leading one', () => {
    expect(artifactExtension('archive.tar.gz')).toBe('gz');
    expect(artifactExtension('Notes.MD')).toBe('md');
    expect(artifactExtension('.gitignore')).toBe('');
    expect(artifactExtension('Dockerfile')).toBe('');
    expect(artifactExtension('api.env.example')).toBe('env.example');
    expect(isArtifactName('api.env.example')).toBe(true);
  });
});

describe('names and sizes', () => {
  it('wants 1 to 200 characters for a file and 1 to 120 for a folder', () => {
    expect(fileNameProblem('')).toMatch(/name/);
    expect(fileNameProblem('a'.repeat(200))).toBeNull();
    expect(fileNameProblem('a'.repeat(201))).toMatch(/200/);
    expect(folderNameProblem('a'.repeat(120))).toBeNull();
    expect(folderNameProblem('a'.repeat(121))).toMatch(/120/);
  });

  it('counts characters, not UTF-16 units', () => {
    expect(fileNameProblem('😀'.repeat(200))).toBeNull();
  });

  it('refuses slashes and control characters', () => {
    expect(fileNameProblem('a/b.md')).toMatch(/slash/);
    expect(fileNameProblem('a\\b.md')).toMatch(/slash/);
    expect(folderNameProblem('tab\there')).toMatch(/control/);
  });

  it('checks the kind and the 1 MiB limit before anything is sent', () => {
    expect(artifactFileProblem({ name: 'a.ts', size: MAX_ARTIFACT_BYTES })).toBeNull();
    expect(artifactFileProblem({ name: 'a.ts', size: MAX_ARTIFACT_BYTES + 1 })).toMatch(/1 MB/);
    expect(artifactFileProblem({ name: 'a.pdf', size: 10 })).toBe(NOT_TEXT_MESSAGE);
  });
});

describe('the folder tree', () => {
  const docs = folder('docs');
  const api = folder('API', docs);
  const guides = folder('guides', docs);
  const deep = folder('v2', api);
  const src = folder('src');
  const ten = folder('Part 10');
  const two = folder('part 2');
  const all = [deep, src, guides, ten, api, docs, two];

  it('nests the flat list, each level by name ignoring case and reading numbers as numbers', () => {
    const tree = buildFolderTree(all);
    expect(tree.map((n) => n.folder.name)).toEqual(['docs', 'part 2', 'Part 10', 'src']);
    expect(tree[0]!.children.map((n) => n.folder.name)).toEqual(['API', 'guides']);
    expect(tree[0]!.children[0]!.children.map((n) => n.folder.name)).toEqual(['v2']);
  });

  it('puts a folder whose parent is not listed at the top', () => {
    const tree = buildFolderTree([api, deep]);
    expect(tree.map((n) => n.folder.name)).toEqual(['API']);
    expect(tree[0]!.children.map((n) => n.folder.name)).toEqual(['v2']);
  });

  it('never loops on a cycle in the data', () => {
    const a = folder('a');
    const b = folder('b', a);
    const looped = { ...a, parent_id: b.id };
    expect(flattenTree(buildFolderTree([looped, b]))).toEqual([]);
    expect(folderPath([looped, b], b.id).map((f) => f.name)).toEqual(['a', 'b']);
  });

  it('flattens in reading order with depths', () => {
    expect(
      flattenTree(buildFolderTree(all)).map(({ folder: f, depth }) => `${depth}:${f.name}`),
    ).toEqual(['0:docs', '1:API', '2:v2', '1:guides', '0:part 2', '0:Part 10', '0:src']);
  });

  it('finds the path from the top, the descendants and a child by name', () => {
    expect(folderPath(all, deep.id).map((f) => f.name)).toEqual(['docs', 'API', 'v2']);
    expect(folderPath(all, 'missing')).toEqual([]);
    expect(folderAndDescendants(all, docs.id)).toEqual(
      new Set([docs.id, api.id, guides.id, deep.id]),
    );
    expect(childFolder(all, docs.id, 'api')).toBe(api);
    expect(childFolder(all, null, 'api')).toBeUndefined();
  });
});

describe('planning a folder upload', () => {
  const file = (name: string, size = 10) => ({ name, size });

  it('makes every folder the paths need, parents first, once each ignoring case', () => {
    const plan = planFolderUpload([
      { file: file('app.ts'), path: 'site/src/app.ts' },
      { file: file('README.md'), path: 'site/README.md' },
      { file: file('util.ts'), path: 'site/Src/lib/util.ts' },
    ]);
    expect(plan.folders).toEqual([['site'], ['site', 'src'], ['site', 'src', 'lib']]);
    expect(plan.files.map((f) => [f.dirs.join('/'), f.file.name])).toEqual([
      ['site/src', 'app.ts'],
      ['site', 'README.md'],
      ['site/Src/lib', 'util.ts'],
    ]);
    expect(plan.skipped).toEqual([]);
  });

  it('skips what the API would refuse, and makes no folder only they needed', () => {
    const plan = planFolderUpload([
      { file: file('logo.png'), path: 'site/img/logo.png' },
      { file: file('big.json', MAX_ARTIFACT_BYTES + 1), path: 'site/big.json' },
      { file: file('index.html'), path: 'site/index.html' },
    ]);
    expect(plan.folders).toEqual([['site']]);
    expect(plan.files.map((f) => f.file.name)).toEqual(['index.html']);
    expect(plan.skipped).toEqual([
      { path: 'site/img/logo.png', reason: NOT_TEXT_MESSAGE },
      { path: 'site/big.json', reason: 'Larger than 1 MB, the most a file may hold' },
    ]);
  });

  it('takes a file without a relative path as one for the open folder', () => {
    const plan = planFolderUpload([{ file: file('notes.md'), path: '' }]);
    expect(plan.folders).toEqual([]);
    expect(plan.files).toEqual([{ file: file('notes.md'), dirs: [] }]);
  });

  it('ignores the leading slash of a dropped entry', () => {
    const plan = planFolderUpload([{ file: file('a.md'), path: '/notes/a.md' }]);
    expect(plan.folders).toEqual([['notes']]);
  });
});
