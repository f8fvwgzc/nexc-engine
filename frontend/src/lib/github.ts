import { ApiError, errorMessage } from '@/lib/api/errors';

/** A GitHub issue or pull request, as an issue links to it. */
export interface GithubRef {
  owner: string;
  repo: string;
  number: number;
  kind: 'issue' | 'pull';
}

const GITHUB_URL =
  /^https?:\/\/(?:www\.)?github\.com\/([A-Za-z0-9-]+)\/([A-Za-z0-9._-]+)\/(issues|pull)\/(\d+)(?:[/?#].*)?$/;

/** The parts of a GitHub issue or pull request URL; null for anything else. */
export function githubRef(url: string): GithubRef | null {
  const match = GITHUB_URL.exec(url.trim());
  if (!match) return null;
  const [, owner = '', repo = '', kind, number = ''] = match;
  return { owner, repo, number: Number(number), kind: kind === 'pull' ? 'pull' : 'issue' };
}

/** How long a link's label is: `owner/repo#12`, `repo#12` on a card, `#12` where space is tight. */
export type GithubLabelForm = 'full' | 'repo' | 'number';

/** What a GitHub link reads as. A URL that is not an issue or pull request reads as `GitHub`. */
export function githubLabel(url: string, form: GithubLabelForm = 'full'): string {
  const ref = githubRef(url);
  if (!ref) return 'GitHub';
  if (form === 'number') return `#${ref.number}`;
  if (form === 'repo') return `${ref.repo}#${ref.number}`;
  return `${ref.owner}/${ref.repo}#${ref.number}`;
}

/** Why the server refused a GitHub URL, as one line to show under the field. */
export function githubUrlError(error: unknown): string {
  const field = error instanceof ApiError ? error.fieldErrors.github_url?.[0] : undefined;
  return field ?? errorMessage(error);
}
