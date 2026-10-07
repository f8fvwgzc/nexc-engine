import { create } from 'zustand';

interface AssistantState {
  open: boolean;
  /** The workspace the shown conversation belongs to. */
  workspaceId: string | null;
  /** The saved conversation shown in the panel; null while a new one has not been saved yet. */
  conversationId: string | null;
  setOpen: (open: boolean) => void;
  /** Opens the panel on a saved conversation, to read it and continue it. */
  show: (workspaceId: string, conversationId: string) => void;
  /** Clears the panel for a new conversation. */
  startNew: () => void;
  /** Records where the server saved the exchange, so the next message continues it. */
  continueIn: (workspaceId: string, conversationId: string) => void;
}

/** The assistant panel: whether it is open and which conversation it shows. */
export const useAssistantStore = create<AssistantState>()((set) => ({
  open: false,
  workspaceId: null,
  conversationId: null,
  setOpen: (open) => set({ open }),
  show: (workspaceId, conversationId) => set({ open: true, workspaceId, conversationId }),
  startNew: () => set({ conversationId: null }),
  continueIn: (workspaceId, conversationId) => set({ workspaceId, conversationId }),
}));
