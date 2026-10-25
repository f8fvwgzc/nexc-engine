import { QueryErrorResetBoundary } from '@tanstack/react-query';
import { Component, type ReactNode } from 'react';

import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer, type PageWidth } from '@/components/custom-ui/page-container';
import { ApiError } from '@/lib/api/errors';

interface QueryErrorBoundaryProps {
  /** What could not be had, e.g. "Couldn’t load the agents". */
  title?: string;
  /**
   * The boundary stands in for a whole page of this width: the error shows in the page's frame,
   * under `header` (the page's own, so it stays on screen), or as the page's heading without one.
   */
  page?: PageWidth;
  header?: ReactNode;
  /** A smaller frame, for a panel or a section of a page. */
  compact?: boolean;
  className?: string;
  children: ReactNode;
}

interface BoundaryProps extends QueryErrorBoundaryProps {
  onReset: () => void;
}

type BoundaryState = { failed: false } | { failed: true; error: unknown };

class Boundary extends Component<BoundaryProps, BoundaryState> {
  state: BoundaryState = { failed: false };

  static getDerivedStateFromError(error: unknown): BoundaryState {
    return { failed: true, error };
  }

  retry = () => {
    // Lets the failed reads ask again when the content mounts anew, instead of rethrowing.
    this.props.onReset();
    this.setState({ failed: false });
  };

  render() {
    if (!this.state.failed) return this.props.children;
    const { error } = this.state;
    // Something that is not there is the route's to say, with its way home.
    if (error instanceof ApiError && error.status === 404) throw error;
    const { title, page, header, compact, className } = this.props;
    const state = (
      <ErrorState
        title={title}
        error={error}
        headingLevel={page !== undefined && !header ? 1 : undefined}
        compact={compact}
        className={className}
        onRetry={this.retry}
      />
    );
    if (page === undefined) return state;
    return (
      <PageContainer width={page}>
        {header}
        {state}
      </PageContainer>
    );
  }
}

/**
 * Catches what a suspending read inside it throws and shows the error in its place, with a way
 * to ask again, so the page's header and the rest of the page stay on screen (the route's error
 * page would replace all of it). Put it around the `Suspense`. A 404 still goes to the route,
 * which says the thing is gone.
 */
export function QueryErrorBoundary(props: QueryErrorBoundaryProps) {
  return (
    <QueryErrorResetBoundary>
      {({ reset }) => <Boundary {...props} onReset={reset} />}
    </QueryErrorResetBoundary>
  );
}
