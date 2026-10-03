import { use } from 'react';

import { ThemeContext, type ThemeContextValue } from '@/app/providers/theme-context';

export function useTheme(): ThemeContextValue {
  const ctx = use(ThemeContext);
  if (!ctx) throw new Error('useTheme must be used inside <ThemeProvider>');
  return ctx;
}
