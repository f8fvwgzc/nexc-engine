import { useEffect } from 'react';

import { useCommandStore } from '@/stores/command-store';

import { isTypingTarget } from '../shortcuts';

/** ⌘K / Ctrl+K toggles the palette; `?` opens the shortcuts dialog. */
export function useGlobalShortcuts() {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const store = useCommandStore.getState();
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        store.setPaletteOpen(!store.paletteOpen);
        return;
      }
      if (event.key === '?' && !isTypingTarget(event.target) && !store.paletteOpen) {
        event.preventDefault();
        store.setShortcutsOpen(true);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
