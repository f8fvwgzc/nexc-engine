import { CompassIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';

export default function NotFoundPage() {
  return (
    <div className="flex min-h-[70svh] flex-1 items-center justify-center p-6">
      <Seo title="Page not found" noIndex />
      <EmptyState
        icon={CompassIcon}
        title="404 — page not found"
        description="The page you are looking for does not exist or has moved."
        className="max-w-md"
        action={
          <Button asChild>
            <Link to="/app">Back to the dashboard</Link>
          </Button>
        }
      />
    </div>
  );
}
