import { create } from 'zustand';

import type { AuthResponse, User } from '@/schemas/auth';

export type AuthStatus = 'booting' | 'authenticated' | 'anonymous';

interface AuthState {
  status: AuthStatus;
  user: User | null;
  /** Access token lives only in memory — never persisted (CONTRACT §3, refresh is cookie-based). */
  accessToken: string | null;
  /** Epoch ms when the access token expires. */
  expiresAt: number | null;
  setSession: (session: AuthResponse) => void;
  setUser: (user: User) => void;
  clearSession: () => void;
}

export const useAuthStore = create<AuthState>()((set) => ({
  status: 'booting',
  user: null,
  accessToken: null,
  expiresAt: null,
  setSession: ({ user, access_token, expires_in }) =>
    set({
      status: 'authenticated',
      user,
      accessToken: access_token,
      expiresAt: Date.now() + expires_in * 1000,
    }),
  setUser: (user) => set({ user }),
  clearSession: () => set({ status: 'anonymous', user: null, accessToken: null, expiresAt: null }),
}));
