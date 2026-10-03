import { useCallback, useState } from 'react';

function read(key: string): boolean {
  try {
    return window.localStorage.getItem(key) === '1';
  } catch {
    return false;
  }
}

/** A dismissed flag persisted per browser (best effort — storage may be unavailable). */
export function useDismissible(key: string): [dismissed: boolean, dismiss: () => void] {
  const [dismissed, setDismissed] = useState(() => read(key));
  const dismiss = useCallback(() => {
    setDismissed(true);
    try {
      window.localStorage.setItem(key, '1');
    } catch {
      // Remains dismissed for this page lifetime only.
    }
  }, [key]);
  return [dismissed, dismiss];
}
