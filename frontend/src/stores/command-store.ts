import type { LucideIcon } from 'lucide-react';
import { create } from 'zustand';

export interface CommandAction {
  id: string;
  label: string;
  icon: LucideIcon;
  shortcut?: string;
  disabled?: boolean;
  run: () => void;
}

interface CommandState {
  paletteOpen: boolean;
  shortcutsOpen: boolean;
  /** Actions contributed by the current page (e.g. the graph canvas: add node, plan, run). */
  pageActions: CommandAction[];
  setPaletteOpen: (open: boolean) => void;
  setShortcutsOpen: (open: boolean) => void;
  setPageActions: (actions: CommandAction[]) => void;
}

export const useCommandStore = create<CommandState>()((set) => ({
  paletteOpen: false,
  shortcutsOpen: false,
  pageActions: [],
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  setShortcutsOpen: (shortcutsOpen) => set({ shortcutsOpen }),
  setPageActions: (pageActions) => set({ pageActions }),
}));
