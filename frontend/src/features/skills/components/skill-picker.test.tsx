import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';

import type { SkillSummary } from '@/schemas/skill';
import { skillSummaries } from '@/test/fixtures';

import { SkillPicker } from './skill-picker';

const { library } = vi.hoisted(() => ({ library: { list: [] as SkillSummary[] } }));

vi.mock('../api', () => ({
  skillsQuery: () => ({
    queryKey: ['skills', 'list'],
    queryFn: () => Promise.resolve(library.list),
  }),
}));

/** Twelve more skills, to fill an agent up to the limit. */
const filler: SkillSummary[] = Array.from({ length: 12 }, (_, i) => ({
  ...skillSummaries[0]!,
  id: `filler-${i}`,
  name: `Filler ${i}`,
  category: 'zz-filler',
}));

function Harness({ initial, onChange }: { initial: string[]; onChange: (ids: string[]) => void }) {
  const [value, setValue] = useState(initial);
  return (
    <SkillPicker
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

function renderPicker(initial: string[] = []) {
  library.list = [...skillSummaries, ...filler];
  const onChange = vi.fn<(ids: string[]) => void>();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <Harness initial={initial} onChange={onChange} />
    </QueryClientProvider>,
  );
  return onChange;
}

describe('SkillPicker', () => {
  it('lists the skills by category with a search, and adds the one picked last', async () => {
    const onChange = renderPicker(['core-evidence-and-sources']);
    fireEvent.click(screen.getByRole('button', { name: 'Add skills, 1 of 12 chosen' }));
    const list = await screen.findByRole('listbox');
    expect(within(list).getByRole('group', { name: 'Finance' })).toBeInTheDocument();
    expect(within(list).getByRole('option', { name: /evidence and sources/i })).toHaveAttribute(
      'aria-checked',
      'true',
    );

    fireEvent.change(screen.getByRole('combobox', { name: 'Search skills' }), {
      target: { value: 'dcf' },
    });
    const options = within(list).getAllByRole('option');
    expect(options).toHaveLength(1);
    fireEvent.click(options[0]!);
    expect(onChange).toHaveBeenLastCalledWith([
      'core-evidence-and-sources',
      'fin-equity-valuation',
    ]);
    // The open list is modal: the chips behind it are hidden from assistive technology.
    expect(screen.getByRole('list', { name: 'Chosen skills', hidden: true })).toHaveTextContent(
      'Equity valuation',
    );
  });

  it('removes a chosen skill from its chip', async () => {
    const onChange = renderPicker(['core-evidence-and-sources', 'fin-equity-valuation']);
    fireEvent.click(await screen.findByRole('button', { name: 'Remove Evidence and sources' }));
    expect(onChange).toHaveBeenLastCalledWith(['fin-equity-valuation']);
  });

  it('takes no more than twelve: the others are disabled until one is removed', async () => {
    renderPicker(filler.map((s) => s.id));
    fireEvent.click(screen.getByRole('button', { name: 'Add skills, 12 of 12 chosen' }));
    const list = await screen.findByRole('listbox');
    expect(within(list).getByRole('option', { name: /equity valuation/i })).toHaveAttribute(
      'aria-disabled',
      'true',
    );
    expect(screen.getByText(/remove one to add another/i)).toBeInTheDocument();
  });
});
