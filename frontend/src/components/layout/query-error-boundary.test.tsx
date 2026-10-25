import { QueryClient, QueryClientProvider, useSuspenseQuery } from '@tanstack/react-query';
import { fireEvent, render, screen } from '@testing-library/react';
import { Component, Suspense, type ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';

import { ApiError } from '@/lib/api/errors';

import { QueryErrorBoundary } from './query-error-boundary';

function Agents({ read }: { read: () => Promise<string> }) {
  const { data } = useSuspenseQuery({ queryKey: ['agents'], queryFn: read });
  return <p>{data}</p>;
}

/** The route's error page, as far as these tests need it. */
class RouteBoundary extends Component<{ children: ReactNode }, { error: unknown }> {
  state = { error: null as unknown };
  static getDerivedStateFromError(error: unknown) {
    return { error };
  }
  render() {
    return this.state.error ? <p>The route says: not found</p> : this.props.children;
  }
}

function renderPage(read: () => Promise<string>) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <RouteBoundary>
        <QueryErrorBoundary
          page="default"
          header={<h1>Agents</h1>}
          title="Couldn’t load the agents"
        >
          <Suspense fallback={<p>Loading</p>}>
            <Agents read={read} />
          </Suspense>
        </QueryErrorBoundary>
      </RouteBoundary>
    </QueryClientProvider>,
  );
}

describe('a failed suspending read', () => {
  it('shows in place under the page’s header, and asks again on Try again', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const read = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error('The server is down'))
      .mockResolvedValueOnce('Researcher');
    renderPage(read);

    expect(await screen.findByRole('alert')).toHaveTextContent('Couldn’t load the agents');
    expect(screen.getByRole('heading', { level: 1, name: 'Agents' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByText('Researcher')).toBeInTheDocument();
    expect(read).toHaveBeenCalledTimes(2);
  });

  it('leaves a 404 to the route, which says the thing is gone', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const gone = new ApiError({ type: 'about:blank', title: 'Not Found', status: 404 });
    renderPage(() => Promise.reject(gone));
    expect(await screen.findByText('The route says: not found')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});
