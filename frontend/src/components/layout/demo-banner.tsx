import { useQuery } from '@tanstack/react-query';
import { FlaskConicalIcon, XIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { Button } from '@/components/ui/button';
import { orchestratorQuery } from '@/features/graphs/api';
import { useDismissible } from '@/hooks/use-dismissible';

/**
 * Floating notice shown while the backend runs the offline `demo` LLM provider. Floating (not
 * in-flow) so it never shifts the layout when the status query resolves.
 */
export function DemoBanner() {
  const { data: demoMode } = useQuery({ ...orchestratorQuery(), select: (s) => s.demo_mode });
  const [dismissed, dismiss] = useDismissible('nexc-demo-banner-dismissed');
  if (!demoMode || dismissed) return null;

  return (
    <div
      role="status"
      className="fixed inset-x-3 bottom-3 z-40 flex items-start gap-3 rounded-xl border border-brand/30 bg-popover/90 p-3 text-sm shadow-lg shadow-brand/10 backdrop-blur-xl motion-safe:animate-fade-up sm:inset-x-auto sm:right-4 sm:bottom-4 sm:max-w-sm sm:items-center"
    >
      <FlaskConicalIcon className="mt-0.5 size-4 shrink-0 text-brand sm:mt-0" aria-hidden />
      <p className="flex-1 text-pretty">
        <span className="font-medium">Demo mode</span> — outputs are simulated.{' '}
        <Link
          to="/app/settings"
          className="font-medium text-brand underline-offset-4 hover:underline"
        >
          Add an API key in Settings
        </Link>{' '}
        for real results.
      </p>
      <Button variant="ghost" size="icon-sm" onClick={dismiss} aria-label="Dismiss demo notice">
        <XIcon />
      </Button>
    </div>
  );
}
