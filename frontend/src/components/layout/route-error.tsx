import { AlertTriangleIcon } from 'lucide-react';
import {
  isRouteErrorResponse,
  Link,
  useLocation,
  useNavigate,
  useRouteError,
} from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { MainLandmark } from '@/components/custom-ui/main-landmark';
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

  const home = (
    <Button asChild>
      <Link to="/app">Go to dashboard</Link>
    </Button>
  );
  return (
    <MainLandmark className="flex min-h-[60svh] flex-1 items-center justify-center p-6">
      <Seo title={title} noIndex />
      {notFound ? (
        <EmptyState
          icon={AlertTriangleIcon}
          title={title}
          headingLevel={1}
          description={description}
          className="max-w-lg"
          action={home}
        />
      ) : (
        // A failed load looks the same whether the page or a panel of it failed.
        <ErrorState
          title={title}
          headingLevel={1}
          error={error}
          description={description}
          onRetry={() => void navigate(location, { replace: true })}
          action={home}
          className="w-full max-w-lg"
        />
      )}
    </MainLandmark>
  );
}
