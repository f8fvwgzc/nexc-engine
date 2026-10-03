import { Helmet } from 'react-helmet-async';

export const APP_NAME = 'nexc-engine';
const DEFAULT_DESCRIPTION =
  'Plan, connect and run knowledge graphs of LLM tasks — live, dependency-aware, and inspectable.';

interface SeoProps {
  /** Page title; rendered as "<title> — nexc-engine". Omit for the bare app name. */
  title?: string;
  description?: string;
  /** Keep the page out of search indexes (authenticated app pages). */
  noIndex?: boolean;
}

function formatTitle(title?: string): string {
  return title ? `${title} — ${APP_NAME}` : APP_NAME;
}

/** Per-page document head: title, description, theme-color and Open Graph tags. */
export function Seo({ title, description = DEFAULT_DESCRIPTION, noIndex = false }: SeoProps) {
  const fullTitle = formatTitle(title);
  return (
    <Helmet prioritizeSeoTags>
      <title>{fullTitle}</title>
      <meta name="description" content={description} />
      <meta name="theme-color" media="(prefers-color-scheme: light)" content="#ffffff" />
      <meta name="theme-color" media="(prefers-color-scheme: dark)" content="#0a0a0b" />
      <meta property="og:site_name" content={APP_NAME} />
      <meta property="og:type" content="website" />
      <meta property="og:title" content={fullTitle} />
      <meta property="og:description" content={description} />
      <meta name="twitter:card" content="summary" />
      {noIndex && <meta name="robots" content="noindex, nofollow" />}
    </Helmet>
  );
}
