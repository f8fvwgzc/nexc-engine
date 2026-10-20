import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { entityPath, ontoEntity, ontoView } from '@/schemas/ontology';

import { OntoFrame } from './onto-frame';

const session = {
  ui_url: 'https://onto.example.com/ui/',
  token: 'onto1.member-session.sig',
  tenant: 'ws_0123',
  workspace: 'nexc-0123',
  expires_at: '2099-01-01T00:00:00Z',
};

vi.mock('./api', () => ({
  ontologySessionQuery: () => ({
    queryKey: ['ontology', 'w', 'session'],
    queryFn: () => Promise.resolve(session),
  }),
}));

function announce(origin: string, source: MessageEventSource | null) {
  act(() => {
    window.dispatchEvent(
      new MessageEvent('message', { data: { type: 'onto:ready' }, origin, source }),
    );
  });
}

describe('the embedded onto-intelli UI', () => {
  it('gets the session only once it announces itself, from its own origin', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <OntoFrame workspaceId="w" view="/map?workspace=nexc-0123" title="Workspace map" />
      </QueryClientProvider>,
    );
    const frame = await screen.findByTitle<HTMLIFrameElement>('Workspace map');
    expect(frame.src).toBe('https://onto.example.com/ui/#/map?workspace=nexc-0123');
    expect(frame.src).not.toContain(session.token);
    const target = frame.contentWindow!;
    const post = vi.spyOn(target, 'postMessage').mockImplementation(() => undefined);

    // Someone else's page, or another frame, gets nothing.
    announce('https://evil.example', target);
    announce('https://onto.example.com', window);
    expect(post).not.toHaveBeenCalled();

    announce('https://onto.example.com', target);
    expect(post).toHaveBeenCalledWith(
      { type: 'onto:session', token: session.token, tenant: 'ws_0123', workspace: 'nexc-0123' },
      'https://onto.example.com',
    );
  });

  it('opens only the views it knows, and names entities as onto-intelli does', () => {
    expect(ontoView('/entities/issue:01a1')).toBe('/entities/issue:01a1');
    expect(ontoView('/proposals/prop_0199')).toBe('/proposals/prop_0199');
    expect(ontoView('javascript:alert(1)')).toBe('/map');
    expect(ontoView('//evil.example/ui')).toBe('/map');
    expect(ontoEntity('agent', 'agent://acme/writer')).toBe('agent:acme/writer');
    expect(ontoEntity('graph', 'g1')).toBe('execution_graph:g1');
    expect(entityPath('agent:acme/writer')).toBe(
      '/app/settings/map?view=%2Fentities%2Fagent%253Aacme%252Fwriter',
    );
  });
});
