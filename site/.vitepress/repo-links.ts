import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type MarkdownIt from 'markdown-it';

/**
 * The site renders the repository's own Markdown, so the links inside it name repository
 * files (`CONTRACT.md`, `docs/ARCHITECTURE.md#scaling-and-state`, `../../docs/...`). This
 * markdown-it rule maps such a link to the page that renders the file, or to the file on
 * GitHub when the site does not render it. A link to a file the repository does not have
 * fails the build, like a dead link to a page.
 */

const REPO_ROOT = fileURLToPath(new URL('../..', import.meta.url));
const GITHUB_BLOB = 'https://github.com/f8fvwgzc/nexc-engine/blob/main/';

/** Repository files the site renders: their path from the repository root, and the page. */
export const ROUTES: Record<string, string> = {
  'README.md': '/',
  'docs/ARCHITECTURE.md': '/concepts/architecture',
  'docs/CONTRACT.md': '/reference/',
  'docs/CONTRACT.md#2-environment-variables-one-env-at-repo-root-see-envexample':
    '/reference/environment',
  'docs/AGENTSECOPS.md': '/secops/assessment',
  'docs/INFRASTRUCTURE.md': '/operate/infrastructure',
  'docs/HA.md': '/operate/ha',
  'docs/adr/0001-http3-edge-caching-and-indexes.md': '/operate/transport',
  'docs/ONTOLOGY.md': '/operate/ontology',
  'deploy/k8s/README.md': '/operate/kubernetes',
  'deploy/helm/nexc-engine/values.yaml': '/operate/helm#values',
  'site/operate/sso.md': '/operate/sso',
  'CONTRIBUTING.md': '/reference/contributing',
  'SECURITY.md': '/operate/security#reporting-a-vulnerability',
  'CHANGELOG.md': '/changelog',
};

/** Where a repository-relative link goes on the site, or `null` for a link to leave alone. */
export function resolveRepoLink(href: string): string | null {
  // Absolute URLs, site paths and anchors are not repository links.
  if (/^(?:[a-z][a-z0-9+.-]*:|\/|#)/i.test(href)) return null;
  const [pathPart = '', anchor] = href.split('#');
  const path = pathPart.replace(/^(?:\.\.?\/)+/, '');
  const matches = (key: string, candidate: string) =>
    key === candidate || key.endsWith(`/${candidate}`);
  if (anchor) {
    const withAnchor = `${path}#${anchor}`;
    for (const [key, route] of Object.entries(ROUTES)) {
      if (matches(key, withAnchor)) return route;
    }
  }
  for (const [key, route] of Object.entries(ROUTES)) {
    if (key.includes('#')) continue;
    if (matches(key, path)) return anchor ? `${route}#${anchor}` : route;
  }
  if (existsSync(resolve(REPO_ROOT, path))) {
    return `${GITHUB_BLOB}${path}${anchor ? `#${anchor}` : ''}`;
  }
  throw new Error(`Link to a file the repository does not have: ${href}`);
}

export function repoLinks(md: MarkdownIt): void {
  md.core.ruler.push('repo_links', (state) => {
    for (const token of state.tokens) {
      if (token.type !== 'inline' || !token.children) continue;
      for (const child of token.children) {
        if (child.type !== 'link_open') continue;
        const href = child.attrGet('href');
        const resolved = href ? resolveRepoLink(href) : null;
        if (resolved) child.attrSet('href', resolved);
      }
    }
  });
}
