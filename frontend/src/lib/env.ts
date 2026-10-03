import { z } from 'zod';

const relativeOrAbsoluteUrl = z
  .string()
  .refine((v) => v.startsWith('/') || /^https?:\/\//.test(v), {
    error: 'must be an absolute URL or a path starting with "/"',
  })
  .transform((v) => v.replace(/\/+$/, ''));

const envSchema = z.object({
  VITE_API_BASE_URL: relativeOrAbsoluteUrl.default('/api/v1'),
  VITE_API_DOCS_URL: relativeOrAbsoluteUrl.default('/api/docs'),
  MODE: z.string(),
  DEV: z.boolean(),
});

const parsed = envSchema.safeParse(import.meta.env);
if (!parsed.success) {
  throw new Error(`Invalid frontend environment:\n${z.prettifyError(parsed.error)}`);
}

export const env = {
  apiBaseUrl: parsed.data.VITE_API_BASE_URL,
  apiDocsUrl: parsed.data.VITE_API_DOCS_URL,
  mode: parsed.data.MODE,
  isDev: parsed.data.DEV,
} as const;

/** Builds an absolute ws(s):// URL for a path under the API base (works with relative bases). */
export function apiWebSocketUrl(path: string): string {
  const url = new URL(`${env.apiBaseUrl}${path}`, window.location.href);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url.toString();
}
