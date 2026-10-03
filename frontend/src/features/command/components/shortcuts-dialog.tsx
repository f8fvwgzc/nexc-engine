import { KbdHint } from '@/components/custom-ui/kbd-hint';
import { Kbd } from '@/components/ui/kbd';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { useCommandStore } from '@/stores/command-store';

import { SHORTCUT_GROUPS } from '../shortcuts';

function Keys({ keys }: { keys: string }) {
  // Gestures ("shift+drag", "double-click") read better as words than keycaps.
  if (keys.includes('drag') || keys.includes('click')) {
    return <Kbd className="font-normal">{keys.replace('+', ' + ')}</Kbd>;
  }
  return <KbdHint keys={keys} className="inline-flex" />;
}

export function ShortcutsDialog() {
  const open = useCommandStore((s) => s.shortcutsOpen);
  const setOpen = useCommandStore((s) => s.setShortcutsOpen);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>Move faster around the workspace and the canvas.</DialogDescription>
        </DialogHeader>
        <div className="space-y-5">
          {SHORTCUT_GROUPS.map((group) => (
            <section key={group.title} aria-labelledby={`shortcuts-${group.title}`}>
              <h3
                id={`shortcuts-${group.title}`}
                className="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase"
              >
                {group.title}
              </h3>
              <ul className="divide-y rounded-lg border">
                {group.items.map((item) => (
                  <li
                    key={item.keys}
                    className="flex items-center justify-between gap-4 px-3 py-2 text-sm"
                  >
                    <span>{item.label}</span>
                    <Keys keys={item.keys} />
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
