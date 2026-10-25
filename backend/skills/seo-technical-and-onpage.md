---
id: seo-technical-and-onpage
name: Technical and on-page SEO
category: marketing
description: Audits crawling, indexing, canonicals, sitemaps, robots.txt, Core Web Vitals, structured data, hreflang and on-page elements against Google Search Central rules, and ranks the fixes.
keywords: seo, technical seo, crawling, indexing, robots.txt, sitemap, canonical, noindex, core web vitals, lcp, inp, cls, structured data, schema, hreflang, site migration
source: Google Search Central - Core Web Vitals and Google Search results | https://developers.google.com/search/docs/appearance/core-web-vitals
source: Google Search Central - How Google interprets the robots.txt specification | https://developers.google.com/search/docs/crawling-indexing/robots/robots_txt
source: Google Search Central - Consolidate duplicate URLs | https://developers.google.com/search/docs/crawling-indexing/consolidate-duplicate-urls
source: Google Search Central - Localized versions of your pages | https://developers.google.com/search/docs/specialty/international/localized-versions
source: web.dev - Interaction to Next Paint (INP) | https://web.dev/articles/inp
---
## When to use
Audits, launches, migrations, indexing drops, international roll-outs and performance work. Work from crawl and Search Console exports, server logs, HTML and config files; you cannot fetch live URLs, so list the checks that need a live crawl.

## Method
1. Discovery: each key URL returns 200, is linked by a crawlable `<a href>` and sits in an XML sitemap. Content, links and canonical must exist in the rendered HTML; Google indexes the mobile version. Permanent moves use one 301/308 hop, no chains; retired pages return 404 or 410.
2. robots.txt controls crawling, not indexing. Google applies the longest matching path; on a tie the least restrictive rule wins. Only the first 500 KiB is read; the file is cached up to 24 h. A 4xx (except 429) means no restrictions; a 5xx stops crawling for 12 h, then the last cached copy applies for 30 days, then, if the site is up, as if no file existed. Never block CSS or JS needed to render.
3. Keep a page out of the index with `noindex` (meta robots, or the `X-Robots-Tag` header for PDFs). The page must stay crawlable so the rule is seen; `noindex` in robots.txt is not supported.
4. Canonical signals, strongest first: permanent redirect, `rel="canonical"` (head or HTTP header, pick one), sitemap inclusion (weak). Use absolute URLs, a self-referencing canonical, internal links to the canonical, one canonical across all methods, no fragments. Never canonicalise with robots.txt; noindex removes the page instead of consolidating it.
5. Sitemaps: at most 50,000 URLs or 50 MB uncompressed per file, grouped by a sitemap index. List only canonical, indexable 200 URLs. `lastmod` counts only when consistently accurate; `priority` and `changefreq` are ignored.
6. Core Web Vitals use field data at the 75th percentile, mobile and desktop separately:

| Metric | Good | Needs improvement | Poor |
|---|---|---|---|
| LCP | <= 2.5 s | <= 4.0 s | > 4.0 s |
| INP (replaced FID, 12 Mar 2024) | <= 200 ms | <= 500 ms | > 500 ms |
| CLS | <= 0.1 | <= 0.25 | > 0.25 |

LCP: cut TTFB, preload or `fetchpriority="high"` the hero image, never lazy-load it. INP = input delay + processing time + presentation delay: split long tasks (> 50 ms), defer third-party scripts. CLS: set width/height or aspect-ratio, reserve ad and embed slots.
7. Structured data: JSON-LD preferred; mark up only visible content, fill required properties, use the most specific schema.org type, test with the Rich Results Test. Misleading markup can draw a manual action; valid markup never guarantees a rich result.
8. hreflang via HTML links, HTTP headers or sitemap (one method). ISO 639-1 language plus optional ISO 3166-1 alpha-2 region (en-GB; never UK or EU); each version lists itself and every alternate with return links; add `x-default`; fully qualified URLs.
9. On-page: unique, descriptive `<title>` and meta description, clear headings, readable URL slugs, descriptive anchor text, alt text on meaningful images. Meta keywords are ignored; word count is not a ranking factor.

## Checklist
- Status codes, redirect chains, orphans, soft 404s
- robots.txt and noindex do not conflict; resources crawlable
- Canonicals, sitemaps and internal links agree
- CWV judged on field data per template, not lab scores
- Structured data valid and matching visible content; hreflang reciprocal

## Output
Audit table: issue, evidence (file and sample URLs), URLs affected, impact (high/medium/low), fix, owner, effort; ordered by impact x reach / effort. Migrations add a redirect map (old URL, new URL, status) and pre- and post-launch checks.

## Pitfalls
- Disallowing a URL to deindex it (it can stay indexed without a snippet)
- Canonical pointing at a redirected, noindexed or 404 URL
- Reporting Lighthouse lab scores as the CWV assessment
