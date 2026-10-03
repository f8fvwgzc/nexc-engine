import { AlertTriangleIcon, RotateCcwIcon } from 'lucide-react';
import {
  isRouteErrorResponse,
  Link,
  useLocation,
  useNavigate,
  useRouteError,
} from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { ApiError, errorMessage } from '@/lib/api/errors';

/** errorElement for every route: friendly message, retry, and a way home. */
export function RouteError() {
  const error = useRouteError();
  const navigate = useNavigate();
  const location = useLocation();

  const notFound =
    (isRouteErrorResponse(error) && error.status === 404) ||
    (error instanceof ApiError && error.status === 404);
  const title = notFound ? 'Not found' : 'Something went wrong';
  const description = notFound
    ? 'This page or resource does not exist, or you no longer have access to it.'
    : isRouteErrorResponse(error)
      ? error.statusText
      : errorMessage(error);

  return (
    <div className="flex min-h-[60svh] items-center justify-center p-6">
      <Seo title={title} noIndex />
      <EmptyState
        icon={AlertTriangleIcon}
        title={title}
        description={description}
        className="max-w-lg"
        action={
          <div className="flex gap-2">
            {!notFound && (
              <Button variant="outline" onClick={() => void navigate(location, { replace: true })}>
                <RotateCcwIcon />
                Try again
              </Button>
            )}
            <Button asChild>
              <Link to="/app">Go to dashboard</Link>
            </Button>
          </div>
        }
      />
    </div>
  );
}
