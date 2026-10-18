import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  AlertDelivery,
  AlertDestination,
  AlertDestinationInput,
  DestinationTest,
  IssuedAlertDestination,
} from '@/schemas/alert-routing';
import type { Workspace } from '@/schemas/workspace';

import { describeFilters } from '@/features/alerting/describe';

import AlertDestinationsSettingsPage from './alert-destinations-page';

const workspace: Workspace = {
  id: '018f0000-0000-7000-8000-000000000001',
  name: 'Acme',
  slug: 'acme',
  role: 'admin',
  member_count: 3,
  created_at: '2026-10-01T00:00:00Z',
};

const siem: AlertDestination = {
  id: '018f0000-0000-7000-8000-000000000010',
  workspace_id: workspace.id,
  name: 'SIEM',
  kind: 'webhook',
  host: 'siem.example.com',
  kinds: ['dlp', 'injection'],
  min_severity: 'medium',
  include_approvals: false,
  enabled: true,
  disabled_reason: '',
  failures: 0,
  last_delivered_at: '2026-10-18T09:00:00Z',
  last_error: '',
  created_by: null,
  created_at: '2026-10-17T09:00:00Z',
  updated_at: '2026-10-17T09:00:00Z',
};

const slack: AlertDestination = {
  ...siem,
  id: '018f0000-0000-7000-8000-000000000011',
  name: 'ops',
  kind: 'slack',
  host: 'hooks.slack.com',
  kinds: [],
  min_severity: 'low',
  include_approvals: true,
  enabled: false,
  disabled_reason: '50 deliveries in a row failed; last: HTTP 500: nope',
  failures: 50,
  last_error: 'HTTP 500: nope',
};

const delivered: AlertDelivery = {
  id: '018f0000-0000-7000-8000-000000000020',
  destination_id: siem.id,
  event: 'alert.raised',
  alert_id: '018f0000-0000-7000-8000-000000000030',
  approval_id: null,
  status: 'delivered',
  attempts: 2,
  next_attempt_at: null,
  last_attempt_at: '2026-10-18T09:00:00Z',
  last_status: 200,
  last_error: '',
  created_at: '2026-10-18T08:59:00Z',
  delivered_at: '2026-10-18T09:00:00Z',
};

const { api, state } = vi.hoisted(() => {
  const state = { destinations: [] as unknown[], deliveries: [] as unknown[] };
  return {
    state,
    api: {
      alertDestinationsQuery: vi.fn((workspaceId: string) => ({
        queryKey: ['alert-destinations', workspaceId],
        queryFn: () => Promise.resolve(state.destinations),
      })),
      alertDeliveriesQuery: vi.fn((workspaceId: string, destinationId: string) => ({
        queryKey: ['alert-destinations', workspaceId, 'deliveries', destinationId],
        queryFn: () => Promise.resolve(state.deliveries),
      })),
      registerAlertDestination:
        vi.fn<(w: string, input: AlertDestinationInput) => Promise<IssuedAlertDestination>>(),
      updateAlertDestination: vi.fn<() => Promise<AlertDestination>>(),
      removeAlertDestination: vi.fn<() => Promise<void>>(),
      testAlertDestination: vi.fn<() => Promise<DestinationTest>>(),
    },
  };
});

vi.mock('@/features/alerting/api', () => api);
vi.mock('@/features/workspaces/use-current-workspace', () => ({
  useCurrentWorkspace: () => ({ current: workspace, all: [workspace] }),
}));

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AlertDestinationsSettingsPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  state.destinations = [siem, slack];
  state.deliveries = [delivered];
  api.registerAlertDestination.mockReset();
  api.testAlertDestination.mockReset();
  api.updateAlertDestination.mockReset();
  api.removeAlertDestination.mockReset();
});

describe('describeFilters', () => {
  it('says what a destination sends', () => {
    expect(describeFilters(siem)).toBe('data leaving, injection from medium up');
    expect(describeFilters(slack)).toBe('every kind, approvals too');
  });
});

describe('AlertDestinationsSettingsPage', () => {
  it('lists destinations with their host, filters and state', async () => {
    renderPage();
    const list = await screen.findByRole('list', { name: 'Alert destinations' });
    expect(list).toHaveTextContent('SIEM');
    expect(list).toHaveTextContent('siem.example.com');
    expect(list).toHaveTextContent('Sends data leaving, injection from medium up');
    expect(list).toHaveTextContent('ops');
    expect(list).toHaveTextContent('switched off');
    expect(list).toHaveTextContent('50 deliveries in a row failed');
    expect(screen.getByRole('switch', { name: 'Disable SIEM' })).toBeChecked();
    expect(screen.getByRole('switch', { name: 'Enable ops' })).not.toBeChecked();
  });

  it('registers a destination and shows the secret once', async () => {
    api.registerAlertDestination.mockResolvedValue({
      destination: { ...siem, id: '018f0000-0000-7000-8000-000000000012', name: 'Splunk' },
      secret: 'nexc_whs_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG',
    });
    renderPage();
    await screen.findByRole('list', { name: 'Alert destinations' });
    fireEvent.click(screen.getByRole('button', { name: 'Add a destination' }));
    const dialog = await screen.findByRole('dialog', { name: 'Add an alert destination' });
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Splunk' } });
    fireEvent.change(screen.getByLabelText('URL'), {
      target: { value: 'https://splunk.example.com/services/collector' },
    });
    fireEvent.click(screen.getByLabelText('Data leaving'));
    fireEvent.click(screen.getByLabelText('Send approvals too'));
    fireEvent.submit(dialog.querySelector('form')!);
    await waitFor(() => expect(api.registerAlertDestination).toHaveBeenCalledTimes(1));
    expect(api.registerAlertDestination).toHaveBeenCalledWith(workspace.id, {
      name: 'Splunk',
      kind: 'webhook',
      url: 'https://splunk.example.com/services/collector',
      kinds: ['dlp'],
      min_severity: 'low',
      include_approvals: true,
    });
    const secret = await screen.findByTestId('secret');
    expect(secret).toHaveTextContent('nexc_whs_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG');
    expect(screen.getByRole('button', { name: 'Copy the secret' })).toBeInTheDocument();
    expect(screen.getByRole('dialog', { name: 'Splunk is set up' })).toHaveTextContent(
      'shown once',
    );
  });

  it('tests a destination and shows what it answered, and opens the log', async () => {
    api.testAlertDestination.mockResolvedValue({
      ok: true,
      status: 200,
      detail: 'the destination answered HTTP 200',
      latency_ms: 42,
    });
    renderPage();
    await screen.findByRole('list', { name: 'Alert destinations' });
    fireEvent.click(screen.getByRole('button', { name: 'Test SIEM' }));
    await waitFor(() =>
      expect(screen.getByRole('status')).toHaveTextContent('Test delivered: HTTP 200 in 42 ms'),
    );
    expect(api.testAlertDestination).toHaveBeenCalledWith(workspace.id, siem.id);
    fireEvent.click(screen.getByRole('button', { name: 'Delivery log of SIEM' }));
    const log = await screen.findByRole('list', { name: 'Delivery log' });
    expect(log).toHaveTextContent('delivered');
    expect(log).toHaveTextContent('alert.raised');
    expect(log).toHaveTextContent('HTTP 200');
    expect(log).toHaveTextContent('2 attempts');
  });

  it('switches a destination on and asks before removing one', async () => {
    api.updateAlertDestination.mockResolvedValue({ ...slack, enabled: true, failures: 0 });
    api.removeAlertDestination.mockResolvedValue(undefined);
    renderPage();
    await screen.findByRole('list', { name: 'Alert destinations' });
    fireEvent.click(screen.getByRole('switch', { name: 'Enable ops' }));
    await waitFor(() =>
      expect(api.updateAlertDestination).toHaveBeenCalledWith(workspace.id, slack.id, {
        enabled: true,
      }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Remove SIEM' }));
    const confirm = await screen.findByRole('alertdialog', { name: 'Remove SIEM?' });
    expect(api.removeAlertDestination).not.toHaveBeenCalled();
    fireEvent.click(confirm.querySelector('button:last-child')!);
    await waitFor(() =>
      expect(api.removeAlertDestination).toHaveBeenCalledWith(workspace.id, siem.id),
    );
  });
});
