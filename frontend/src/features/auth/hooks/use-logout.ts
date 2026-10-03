import { useMutation } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';

import { useAuthStore } from '@/stores/auth-store';

import { logout } from '../api';

/** Revokes the refresh family server-side, then drops the in-memory session and query cache. */
export function useLogout() {
  const navigate = useNavigate();
  return useMutation({
    mutationFn: logout,
    meta: { errorToast: false },
    onSettled: () => {
      useAuthStore.getState().clearSession();
      void navigate('/login', { replace: true });
    },
  });
}
