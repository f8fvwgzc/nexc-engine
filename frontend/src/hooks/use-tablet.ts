import { useSyncExternalStore } from 'react';

/** Wide enough for the desktop sidebar's rail, too narrow for the whole of it beside a page. */
const TABLET_QUERY = '(min-width: 768px) and (max-width: 1023.98px)';

function subscribe(onChange: () => void): () => void {
  const media = window.matchMedia(TABLET_QUERY);
  media.addEventListener('change', onChange);
  return () => media.removeEventListener('change', onChange);
}

/** Whether the window is tablet-wide (768–1023 px): the sidebar shows as an icon rail there. */
export function useIsTablet(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => window.matchMedia(TABLET_QUERY).matches,
    () => false,
  );
}
