import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { TooltipProvider } from '@/components/ui/tooltip';
import type { Evidence } from '@/schemas/evidence';
import { ids } from '@/test/fixtures';

import { citationHref, parseCitations } from '../evidence';
import { CitationChips, EvidenceBadge, EvidencePanel } from './evidence';

const chunk = '0190a1b2-c3d4-7e5f-8a9b-cc1d2e3f4a5b';
const memory = ids.memory;
const node = ids.nodeB;

describe('parseCitations', () => {
  it('reads the ids the backend reads, once each, outside code', () => {
    const text = `Revenue grew [doc:${chunk}]. Costs fell. [mem:${memory}] Both [doc:${chunk}, node:${node}].
See \`[doc:${chunk}]\` and [the report](http://x) and [1] and [customs.pdf, p. 17].
\`\`\`
[node:${node}] not a citation
[fact:document:0199] not either
\`\`\`
Something is certain [assumption].`;
    expect(parseCitations(text).map((c) => c.key)).toEqual([
      `doc:${chunk}`,
      `mem:${memory}`,
      `node:${node}`,
    ]);
    expect(parseCitations('nothing here')).toEqual([]);
    expect(parseCitations('[fact:document:0199] rests on the audit')[0]?.id).toBe('document:0199');
  });

  it('links a chip to what it points at', () => {
    const [doc, mem, up, fact] = parseCitations(
      `[doc:${chunk}] [mem:${memory}] [node:${node}] [fact:run:0199]`,
    );
    expect(citationHref(doc!, {})).toBe(`/app/settings/knowledge?passage=${chunk}`);
    expect(citationHref(mem!, {})).toBe(`/app/settings/memory?memory=${memory}`);
    expect(citationHref(up!, { graphId: ids.graph })).toBe(`/app/graphs/${ids.graph}?node=${node}`);
    expect(citationHref(up!, { runId: ids.run })).toBe(`/app/runs/${ids.run}`);
    expect(citationHref(fact!, {})).toBe('/app/settings/audit?fact=run%3A0199');
  });
});

const evidence: Evidence = {
  verdict: 'partial',
  coverage: 0.5,
  claims: 5,
  cited_claims: 2,
  declared_claims: 1,
  cited: [`doc:${chunk}`, `node:${node}`],
  unknown: [`mem:${memory}`],
  given: 3,
  min_coverage: 0.8,
};

describe('evidence components', () => {
  it('renders the chips as links and crosses out what was never given', () => {
    render(
      <MemoryRouter>
        <CitationChips
          cited={evidence.cited}
          unknown={evidence.unknown}
          graphId={ids.graph}
          titles={new Map([[node, 'Outline']])}
        />
      </MemoryRouter>,
    );
    const chips = screen.getByRole('list', { name: 'Citations' });
    const links = chips.querySelectorAll('a');
    expect(links).toHaveLength(2);
    expect(links[0]?.getAttribute('href')).toBe(`/app/settings/knowledge?passage=${chunk}`);
    expect(links[1]?.getAttribute('href')).toBe(`/app/graphs/${ids.graph}?node=${node}`);
    expect(links[1]?.textContent).toContain('Outline');
    const forged = chips.querySelector('[data-unknown]');
    expect(forged?.textContent).toContain(`mem:${memory.slice(0, 8)}`);
    expect(forged?.tagName).toBe('SPAN');
  });

  it('shows the verdict with its coverage and lists the unknown ids', () => {
    render(
      <TooltipProvider>
        <MemoryRouter>
          <EvidencePanel evidence={evidence} graphId={ids.graph} />
        </MemoryRouter>
      </TooltipProvider>,
    );
    const badge = screen.getByText('Partial');
    expect(badge.closest('[data-verdict]')?.getAttribute('data-verdict')).toBe('partial');
    expect(badge.closest('[data-verdict]')?.textContent).toContain('50%');
    expect(screen.getByText(/2 of 4 claims cited/)).toBeInTheDocument();
    expect(screen.getByRole('note').textContent).toContain(`mem:${memory}`);
    render(
      <TooltipProvider>
        <EvidenceBadge evidence={{ ...evidence, verdict: 'verified', coverage: 1, unknown: [] }} />
      </TooltipProvider>,
    );
    expect(screen.getByText('Verified').closest('[data-verdict]')?.textContent).toContain('100%');
  });
});
