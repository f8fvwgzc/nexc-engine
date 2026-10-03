import type { z } from 'zod';

import { problemSchema, type Problem } from '@/schemas/problem';

/** An RFC 7807 problem returned by the backend (or synthesised for non-JSON failures). */
export class ApiError extends Error {
  readonly status: number;
  readonly title: string;
  readonly detail: string | null;
  readonly fieldErrors: Record<string, string[]>;
  /** Seconds to wait before retrying (429 `Retry-After`). */
  readonly retryAfter: number | null;

  constructor(problem: Problem, retryAfter: number | null = null) {
    super(problem.detail ?? problem.title);
    this.name = 'ApiError';
    this.status = problem.status;
    this.title = problem.title;
    this.detail = problem.detail ?? null;
    this.fieldErrors = problem.errors ?? {};
    this.retryAfter = retryAfter;
  }

  static async fromResponse(res: Response): Promise<ApiError> {
    const retryAfterHeader = res.headers.get('Retry-After');
    const retryAfter = retryAfterHeader ? Number.parseInt(retryAfterHeader, 10) : null;
    let body: unknown = null;
    try {
      body = await res.json();
    } catch {
      // Non-JSON error body (proxy error page, empty 502…) — fall through to a synthetic problem.
    }
    const parsed = problemSchema.safeParse(body);
    const problem: Problem = parsed.success
      ? parsed.data
      : { type: 'about:blank', title: res.statusText || 'Request failed', status: res.status };
    return new ApiError(problem, Number.isFinite(retryAfter) ? retryAfter : null);
  }

  /** One human-readable line suitable for a toast. */
  get userMessage(): string {
    if (this.status === 429) {
      return this.retryAfter
        ? `Too many requests — try again in ${this.retryAfter}s.`
        : 'Too many requests — slow down a little.';
    }
    if (this.status === 0) return 'Cannot reach the server. Check your connection.';
    return this.detail && this.detail !== this.title ? `${this.title}: ${this.detail}` : this.title;
  }
}

/** The server answered, but the body did not match the contract schema. */
export class ContractError extends Error {
  readonly issues: z.core.$ZodIssue[];

  constructor(path: string, issues: z.core.$ZodIssue[]) {
    super(`Unexpected response shape from ${path}`);
    this.name = 'ContractError';
    this.issues = issues;
  }
}

export function networkError(cause: unknown): ApiError {
  const error = new ApiError({ type: 'about:blank', title: 'Network error', status: 0 });
  error.cause = cause;
  return error;
}

export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.userMessage;
  if (error instanceof ContractError) return 'The server sent an unexpected response.';
  if (error instanceof Error) return error.message;
  return 'Something went wrong.';
}
