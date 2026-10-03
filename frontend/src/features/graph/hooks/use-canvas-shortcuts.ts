import { useEffect, useEffectEvent } from 'react';

import { isTypingTarget } from '@/features/command/shortcuts';
import { useGraphStore } from '@/stores/graph-store';

interface CanvasShortcuts {
  addNode: () => void;
  fit: () => void;
  autoLayout: () => void;
  requestDelete: (target: { kind: 'node' | 'edge'; id: string }) => void;
}

/** n / f / l / Delete / Esc on the canvas page (ignored while typing or when a dialog is open). */
export function useCanvasShortcuts(actions: CanvasShortcuts) {
  const onKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
    if (isTypingTarget(event.target) || document.querySelector('[role="dialog"]')) return;
    const store = useGraphStore.getState();
    switch (event.key) {
      case 'n':
        actions.addNode();
        break;
      case 'f':
        actions.fit();
        break;
      case 'l':
        actions.autoLayout();
        break;
      case 'Delete':
      case 'Backspace':
        if (store.selectedNodeId) actions.requestDelete({ kind: 'node', id: store.selectedNodeId });
        else if (store.selectedEdgeId)
          actions.requestDelete({ kind: 'edge', id: store.selectedEdgeId });
        else return;
        break;
      case 'Escape':
        store.selectNode(null);
        break;
      default:
        return;
    }
    event.preventDefault();
  });

  useEffect(() => {
    const listener = (event: KeyboardEvent) => onKeyDown(event);
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  }, []);
}
