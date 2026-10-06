import type { z } from 'zod';

import { env } from '@/lib/env';
import { useAuthStore } from '@/stores/auth-store';

import { ApiError, ContractError, networkError } from './errors';
import { refreshSession } from './session';

type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
type QueryValue = string | number | boolean | null | undefined;

export interface RequestOptions {
  method?: HttpMethod;
  body?: unknown;
  query?: Record<string, QueryValue>;
  headers?: Record<string, string>;
  signal?: AbortSignal;
  /** Attach the bearer token and auto-refresh on 401 (default true). */
  auth?: boolean;
}

export function buildUrl(path: string, query?: Record<string, QueryValue>): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value !== undefined && value !== null && value !== '') search.set(key, String(value));
  }
  const qs = search.toString();
  return `${env.apiBaseUrl}${path}${qs ? `?${qs}` : ''}`;
}

async function send(path: string, opts: RequestOptions, token: string | null): Promise<Response> {
  const headers: Record<string, string> = { Accept: 'application/json', ...opts.headers };
  // A file goes as it is; anything else is JSON.
  const raw = opts.body instanceof Blob;
  if (raw) headers['Content-Type'] = 'application/octet-stream';
  else if (opts.body !== undefined) headers['Content-Type'] = 'application/json';
  if (token) headers.Authorization = `Bearer ${token}`;
  try {
    return await fetch(buildUrl(path, opts.query), {
      method: opts.method ?? 'GET',
      headers,
      body: raw
        ? (opts.body as Blob)
        : opts.body === undefined
          ? undefined
          : JSON.stringify(opts.body),
      credentials: 'include',
      signal: opts.signal,
    });
  } catch (cause) {
    if (cause instanceof DOMException && cause.name === 'AbortError') throw cause;
    throw networkError(cause);
  }
}

/** Sends a request; on 401 refreshes the session once (single-flight) and retries. */
async function fetchWithAuth(path: string, opts: RequestOptions): Promise<Response> {
  const useAuth = opts.auth ?? true;
  const token = useAuth ? useAuthStore.getState().accessToken : null;
  const res = await send(path, opts, token);
  if (res.status !== 401 || !useAuth) return res;

  const session = await refreshSession();
  if (!session) return res;
  return send(path, opts, session.access_token);
}

function parseBody<S extends z.ZodType>(schema: S, data: unknown, path: string): z.output<S> {
  const result = schema.safeParse(data);
  if (!result.success) throw new ContractError(path, result.error.issues);
  return result.data;
}

/** JSON request whose response is validated against `schema` (the runtime contract guard). */
export async function apiRequest<S extends z.ZodType>(
  path: string,
  schema: S,
  opts: RequestOptions = {},
): Promise<z.output<S>> {
  const res = await fetchWithAuth(path, opts);
  if (!res.ok) throw await ApiError.fromResponse(res);
  return parseBody(schema, await res.json(), path);
}

/** Request that answers 204 / an ignored body. */
export async function apiSend(path: string, opts: RequestOptions = {}): Promise<void> {
  const res = await fetchWithAuth(path, opts);
  if (!res.ok) throw await ApiError.fromResponse(res);
}

/** Authenticated binary download; returns the blob plus the server-suggested filename. */
export async function apiDownload(
  path: string,
  opts: RequestOptions = {},
): Promise<{ blob: Blob; filename: string | null }> {
  const res = await fetchWithAuth(path, {
    ...opts,
    headers: { Accept: '*/*', ...opts.headers },
  });
  if (!res.ok) throw await ApiError.fromResponse(res);
  return { blob: await res.blob(), filename: filenameFromDisposition(res.headers) };
}

export function filenameFromDisposition(headers: Headers): string | null {
  const disposition = headers.get('Content-Disposition');
  if (!disposition) return null;
  const encoded = /filename\*=UTF-8''([^;]+)/i.exec(disposition);
  if (encoded?.[1]) {
    try {
      return decodeURIComponent(encoded[1]);
    } catch {
      // fall back to the plain filename parameter
    }
  }
  const plain = /filename="?([^";]+)"?/i.exec(disposition);
  return plain?.[1] ?? null;
}
