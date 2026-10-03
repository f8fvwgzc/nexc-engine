import { useMutation } from '@tanstack/react-query';
import type { FieldPath, FieldValues, UseFormSetError } from 'react-hook-form';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { ApiError, errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import { safeNextPath } from '@/lib/utils';
import type { AuthResponse } from '@/schemas/auth';
import { useAuthStore } from '@/stores/auth-store';

interface Options<T extends FieldValues> {
  submit: (values: T) => Promise<AuthResponse>;
  setError: UseFormSetError<T>;
  fields: readonly FieldPath<T>[];
  /** Message for statuses that have a friendlier meaning on this form (e.g. 401, 403). */
  statusMessages?: Partial<Record<number, string>>;
}

/** Shared login/register submit: stores the session, maps problem+json onto the form, redirects. */
export function useAuthSubmit<T extends FieldValues>({
  submit,
  setError,
  fields,
  statusMessages = {},
}: Options<T>) {
  const navigate = useNavigate();
  const [params] = useSearchParams();

  return useMutation({
    mutationFn: submit,
    meta: { errorToast: false },
    onSuccess: (session) => {
      useAuthStore.getState().setSession(session);
      void navigate(safeNextPath(params.get('next')), { replace: true });
    },
    onError: (error) => {
      if (applyProblemToForm(error, setError, fields)) return;
      const friendly = error instanceof ApiError ? statusMessages[error.status] : undefined;
      setError('root.server', {
        type: 'server',
        message: friendly ?? errorMessage(error),
      });
    },
  });
}
