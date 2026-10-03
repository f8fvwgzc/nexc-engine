import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';

import { ApiError, ContractError, errorMessage } from '@/lib/api/errors';

declare module '@tanstack/react-query' {
  interface Register {
    mutationMeta: {
      /** Set false when the caller renders the error itself (e.g. maps it onto form fields). */
      errorToast?: boolean;
      successMessage?: string;
    };
  }
}

function shouldRetry(failureCount: number, error: unknown): boolean {
  if (error instanceof ContractError) return false;
  if (error instanceof ApiError && error.status >= 400 && error.status < 500) return false;
  return failureCount < 2;
}

export function createQueryClient(): QueryClient {
  return new QueryClient({
    queryCache: new QueryCache({
      // Initial-load errors surface through route error boundaries; only toast background failures.
      onError: (error, query) => {
        if (query.state.data !== undefined) toast.error(errorMessage(error));
      },
    }),
    mutationCache: new MutationCache({
      onSuccess: (_data, _vars, _ctx, mutation) => {
        const message = mutation.meta?.successMessage;
        if (message) toast.success(message);
      },
      onError: (error, _vars, _ctx, mutation) => {
        if (mutation.meta?.errorToast !== false) toast.error(errorMessage(error));
      },
    }),
    defaultOptions: {
      queries: {
        staleTime: 30_000,
        gcTime: 10 * 60_000,
        retry: shouldRetry,
        refetchOnWindowFocus: true,
      },
      mutations: { retry: false },
    },
  });
}

export const queryClient = createQueryClient();
