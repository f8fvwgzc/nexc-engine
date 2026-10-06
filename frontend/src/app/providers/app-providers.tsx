import { QueryClientProvider } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { HelmetProvider } from 'react-helmet-async';

import { Toaster } from '@/components/ui/sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { queryClient } from '@/lib/query-client';

import { AuthBootstrap } from './auth-bootstrap';
import { ThemeProvider } from './theme-provider';

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <HelmetProvider>
      <ThemeProvider>
        <QueryClientProvider client={queryClient}>
          <AuthBootstrap>
            <TooltipProvider delayDuration={250}>
              {children}
              {/* Toasts sit above the assistant button, which owns the bottom-right corner. */}
              <Toaster
                richColors
                closeButton
                position="bottom-right"
                offset={{ bottom: 76 }}
                mobileOffset={{ bottom: 76 }}
              />
            </TooltipProvider>
          </AuthBootstrap>
        </QueryClientProvider>
      </ThemeProvider>
    </HelmetProvider>
  );
}
