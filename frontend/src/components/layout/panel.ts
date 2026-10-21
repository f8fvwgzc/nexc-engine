import { type CSSProperties, useState } from 'react';

import { useIsTablet } from '@/hooks/use-tablet';

/** The rail (3rem) and the panel beside it. Collapsed, only the rail is left. */
export const SHELL_STYLE = { '--sidebar-width': '17rem' } as CSSProperties;

/**
 * Whether the navigation panel is open beside the rail: on desktop it is, on a tablet the page
 * needs the width, until the member says otherwise (⌘B, or the header's button).
 */
export function usePanelOpen(): [boolean, (open: boolean) => void] {
  const tablet = useIsTablet();
  const [choice, setChoice] = useState<boolean | null>(null);
  return [choice ?? !tablet, setChoice];
}
