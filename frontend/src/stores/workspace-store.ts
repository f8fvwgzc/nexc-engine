import { create } from 'zustand';

const STORAGE_KEY = 'nexc.workspace';

function remembered(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

interface WorkspaceState {
  /** The workspace the user is working in; null until their workspaces are known. */
  currentId: string | null;
  setCurrent: (id: string) => void;
}

/** Which workspace is open. Remembered per browser so a reload lands in the same place. */
export const useWorkspaceStore = create<WorkspaceState>()((set) => ({
  currentId: remembered(),
  setCurrent: (id) => {
    try {
      localStorage.setItem(STORAGE_KEY, id);
    } catch {
      // Private mode: the choice lasts for this tab only.
    }
    set({ currentId: id });
  },
}));
