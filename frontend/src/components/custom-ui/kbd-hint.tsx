import { Kbd, KbdGroup } from '@/components/ui/kbd';
import { cn } from '@/lib/utils';

const isMac =
  typeof navigator !== 'undefined' &&
  /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

const LABELS: Record<string, string> = {
  mod: isMac ? '⌘' : 'Ctrl',
  shift: '⇧',
  alt: isMac ? '⌥' : 'Alt',
  enter: '↵',
  delete: isMac ? '⌫' : 'Del',
  esc: 'Esc',
};

/** Renders a shortcut like `"mod+k"` as platform-aware keycaps. */
export function KbdHint({ keys, className }: { keys: string; className?: string }) {
  return (
    <KbdGroup className={cn('hidden sm:inline-flex', className)}>
      {keys.split('+').map((key) => (
        <Kbd key={key}>{LABELS[key] ?? key.toUpperCase()}</Kbd>
      ))}
    </KbdGroup>
  );
}
