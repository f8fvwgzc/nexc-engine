import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Infrastructure, LatencyReport } from '@/schemas/insight';
import { useAuthStore } from '@/stores/auth-store';
import * as f from '@/test/fixtures';

import InfrastructurePage from './infrastructure-page';

const infrastructure: Infrastructure = {
  database: {
    location: 'postgres://postgres:5432/nexc',
    version: 'PostgreSQL 17.2',
    size_bytes: 12_000_000,
    pgvector: null,
    migrations_applied: 40,
    migrations_known: 40,
  },
  runtime_url: 'http://agent-runtime:8090',
  runtime_reachable: true,
  runtime_protocol: 'grpc',
  runtime_grpc: { enabled: true, ok: true, url: 'http://agent-runtime:8095', detail: null },
  firewall: { enabled: false, addr: null, public_key: 'ab'.repeat(32), open_sessions: 0 },
  otlp: {
    enabled: false,
    endpoint: null,
    reachable: false,
    spans_exported: 0,
    behind: 0,
    last_exported_at: null,
    last_error: null,
  },
  gateway: { calls_per_minute: 120, timeout_secs: 30, max_response_bytes: 1_048_576 },
  embedding_model: 'builtin-hash-256',
  ontology: { enabled: false, ok: false, url: null, detail: 'disabled' },
  queue: 'PostgreSQL (rows claimed with SKIP LOCKED)',
  cache: 'in process, per server instance',
};

const { api, state } = vi.hoisted(() => {
  const state = { latency: { window_secs: 300, rows: [] } as unknown };
  return {
    state,
    api: {
      infrastructureQuery: vi.fn(() => ({
        queryKey: ['insight', 'infrastructure'],
        queryFn: () => Promise.resolve(infrastructure),
      })),
      latencyQuery: vi.fn(() => ({
        queryKey: ['insight', 'latency'],
        queryFn: () => Promise.resolve(state.latency),
      })),
      checkConnection: vi.fn(),
    },
  };
});

vi.mock('@/features/insight/api', () => api);

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <InfrastructurePage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  useAuthStore.getState().setSession({ ...f.authResponse, user: { ...f.user, role: 'admin' } });
});
afterEach(() => useAuthStore.getState().clearSession());

describe('InfrastructurePage latency panel', () => {
  it('lists each endpoint with its calls, p50, p99 and max over the window', async () => {
    const report: LatencyReport = {
      window_secs: 300,
      rows: [
        {
          method: 'GET',
          route: '/api/v1/issues/{iid}',
          count: 120,
          p50_ms: 3.5,
          p99_ms: 12.4,
          max_ms: 140.2,
        },
        {
          method: 'gRPC',
          route: 'IntelliService/Ask',
          count: 8,
          p50_ms: 0.21,
          p99_ms: 0.9,
          max_ms: 1.1,
        },
      ],
    };
    state.latency = report;
    renderPage();
    const panel = await screen.findByRole('region', { name: 'Latency' });
    expect(panel).toHaveTextContent('Latency, last 5 minutes');
    const rows = panel.querySelectorAll('tbody tr');
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent('GET /api/v1/issues/{iid}');
    expect(rows[0]).toHaveTextContent('120');
    expect(rows[0]).toHaveTextContent('3.50');
    expect(rows[0]).toHaveTextContent('12.4');
    expect(rows[0]).toHaveTextContent('140');
    expect(rows[1]).toHaveTextContent('gRPC IntelliService/Ask');
    expect(rows[1]).toHaveTextContent('0.21');
  });

  it('says so when nothing was served in the window', async () => {
    state.latency = { window_secs: 300, rows: [] };
    renderPage();
    const panel = await screen.findByRole('region', { name: 'Latency' });
    expect(panel).toHaveTextContent('Nothing answered in the window yet.');
  });

  it('shows nothing of the server to a workspace member', () => {
    useAuthStore.getState().setSession(f.authResponse);
    renderPage();
    expect(screen.queryByRole('region', { name: 'Latency' })).toBeNull();
    expect(
      screen.getByText('Only administrators of this server see its infrastructure'),
    ).toBeInTheDocument();
  });
});
