import { cpSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'vitepress';

import { mermaidFence } from './mermaid';
import { repoLinks } from './repo-links';

// The screenshots and the favicon stay where the README and the app keep them; they are copied
// into `public/` (git-ignored there) whenever the site is built or served.
const site = fileURLToPath(new URL('..', import.meta.url));
cpSync(resolve(site, '../docs/media'), resolve(site, 'public/media'), { recursive: true });
cpSync(resolve(site, '../frontend/public/favicon.svg'), resolve(site, 'public/favicon.svg'));

// GitHub Pages serves a project site under /<repository>/; the Pages workflow passes the path it
// was given. `DOCS_BASE=/` serves the site at a root (a custom domain, `make docs`).
const base = process.env.DOCS_BASE ?? '/nexc-engine/';

export default defineConfig({
  title: 'nexc-engine',
  description:
    'Sketch a goal as a graph, let an LLM plan it, run it with agents you can identify, trace, defend and audit.',
  base,
  cleanUrls: true,
  lastUpdated: false,
  // The site's own README is for contributors, not a page.
  srcExclude: ['README.md'],
  // A link to a page that does not exist fails the build; links to a local server do not.
  ignoreDeadLinks: 'localhostLinks',
  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: `${base}favicon.svg` }]],
  sitemap: { hostname: new URL(base, 'https://f8fvwgzc.github.io').href },

  markdown: {
    config(md) {
      md.use(repoLinks);
      md.use(mermaidFence);
    },
  },

  themeConfig: {
    siteTitle: 'nexc-engine',
    nav: [
      { text: 'Start', link: '/start/', activeMatch: '^/start/' },
      { text: 'Concepts', link: '/concepts/', activeMatch: '^/concepts/' },
      { text: 'AgentSecOps', link: '/secops/', activeMatch: '^/secops/' },
      { text: 'Operate', link: '/operate/', activeMatch: '^/operate/' },
      { text: 'Reference', link: '/reference/', activeMatch: '^/reference/' },
      { text: 'Changelog', link: '/changelog', activeMatch: '^/changelog' },
    ],
    sidebar: [
      {
        text: 'Start',
        collapsed: true,
        items: [
          { text: 'Overview', link: '/start/' },
          { text: 'Install in 60 seconds', link: '/start/install' },
          { text: 'Demo mode', link: '/start/demo' },
          { text: 'Claude and local models', link: '/start/models' },
        ],
      },
      {
        text: 'Concepts',
        collapsed: true,
        items: [
          { text: 'Overview', link: '/concepts/' },
          { text: 'Architecture', link: '/concepts/architecture' },
          { text: 'Graphs and plans', link: '/concepts/graphs' },
          { text: 'The ontology', link: '/concepts/ontology' },
          { text: 'Agents', link: '/concepts/agents' },
          { text: 'Memory', link: '/concepts/memory' },
          { text: 'Knowledge', link: '/concepts/knowledge' },
        ],
      },
      {
        text: 'AgentSecOps',
        collapsed: true,
        items: [
          { text: 'Overview', link: '/secops/' },
          { text: 'Assessment and target architecture', link: '/secops/assessment' },
          { text: 'Identity', link: '/secops/identity' },
          { text: 'Trace', link: '/secops/trace' },
          { text: 'Policy and defend', link: '/secops/policy' },
          { text: 'Ask the audit', link: '/secops/audit' },
          { text: 'Tools gateway', link: '/secops/gateway' },
          { text: 'Agent Firewall API', link: '/secops/firewall' },
          { text: 'A2A on the firewall', link: '/secops/a2a' },
          { text: 'The console', link: '/secops/console' },
          { text: 'Five-minute demo', link: '/secops/demo' },
          { text: 'Data model, protocol and plan', link: '/secops/plan' },
        ],
      },
      {
        text: 'Operate',
        collapsed: true,
        items: [
          { text: 'Overview', link: '/operate/' },
          { text: 'Docker Compose', link: '/operate/compose' },
          { text: 'Kubernetes', link: '/operate/kubernetes' },
          { text: 'Helm chart', link: '/operate/helm' },
          { text: 'High availability', link: '/operate/ha' },
          { text: 'Backups', link: '/operate/backups' },
          { text: 'Infrastructure', link: '/operate/infrastructure' },
          { text: 'Workspace transfers', link: '/operate/transfers' },
          { text: 'Alerts', link: '/operate/alerts' },
          { text: 'SIEM export', link: '/operate/siem' },
          { text: 'OpenTelemetry export', link: '/operate/otlp' },
          { text: 'texc-symphony', link: '/operate/symphony' },
          { text: 'Security', link: '/operate/security' },
          { text: 'Troubleshooting', link: '/operate/troubleshooting' },
        ],
      },
      {
        text: 'Reference',
        collapsed: true,
        items: [
          { text: 'The contract', link: '/reference/' },
          { text: 'Environment variables', link: '/reference/environment' },
          { text: 'Types', link: '/reference/types' },
          { text: 'Endpoints', link: '/reference/endpoints' },
          { text: 'Events', link: '/reference/events' },
          { text: 'Runtime protocol', link: '/reference/runtime' },
          { text: 'texc-symphony bridge', link: '/reference/symphony' },
          { text: 'A2A adapter', link: '/reference/a2a' },
          { text: 'API reference', link: '/reference/api' },
          { text: 'CLI', link: '/reference/cli' },
        ],
      },
      {
        text: 'Project',
        collapsed: true,
        items: [
          { text: 'Changelog', link: '/changelog' },
          { text: 'Contributing', link: '/reference/contributing' },
          { text: 'Security policy', link: '/operate/security#reporting-a-vulnerability' },
        ],
      },
    ],
    outline: { level: [2, 3] },
    search: { provider: 'local', options: { detailedView: true } },
    socialLinks: [{ icon: 'github', link: 'https://github.com/f8fvwgzc/nexc-engine' }],
    externalLinkIcon: true,
    footer: {
      message: 'Released under the MIT License.',
      copyright: 'The nexc-engine contributors',
    },
  },
});
