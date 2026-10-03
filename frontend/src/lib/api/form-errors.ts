import type { FieldValues, Path, UseFormSetError } from 'react-hook-form';

import { ApiError } from './errors';

/**
 * Maps problem+json `errors` onto react-hook-form fields. Returns true when at least one
 * field received an error (so the caller can skip the generic toast).
 */
export function applyProblemToForm<T extends FieldValues>(
  error: unknown,
  setError: UseFormSetError<T>,
  fields: readonly Path<T>[],
): boolean {
  if (!(error instanceof ApiError)) return false;
  let applied = false;
  for (const [field, messages] of Object.entries(error.fieldErrors)) {
    const match = fields.find((f) => f === field);
    const message = messages[0];
    if (match && message) {
      setError(match, { type: 'server', message }, { shouldFocus: !applied });
      applied = true;
    }
  }
  return applied;
}
