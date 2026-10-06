export interface ShortcutGroup {
  title: string;
  items: { keys: string; label: string }[];
}

/** Single source for the `?` dialog; key strings use KbdHint syntax ("mod+k"). */
export const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    title: 'General',
    items: [
      { keys: 'mod+k', label: 'Open command palette' },
      { keys: '?', label: 'Show keyboard shortcuts' },
    ],
  },
  {
    title: 'Graph canvas',
    items: [
      { keys: 'n', label: 'Add a node at the center' },
      { keys: 'f', label: 'Fit graph to view' },
      { keys: 'l', label: 'Auto-layout by dependency level' },
      { keys: 'delete', label: 'Delete the selected node or edge' },
      { keys: 'esc', label: 'Clear selection / close panel' },
      { keys: 'shift+drag', label: 'Drag from one node to another to add a dependency' },
      { keys: 'double-click', label: 'Create a node at that spot' },
    ],
  },
];

/** True when a keyboard event originates from a text-editing element. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.tagName === 'SELECT'
  );
}
