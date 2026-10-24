import { describe, expect, it } from 'vitest';

import { skillSummaries } from '@/test/fixtures';

import { categoryLabel, groupByCategory, skillMatches } from './categories';

describe('skill categories', () => {
  it('names a category in words, acronyms included', () => {
    expect(categoryLabel('supply-chain')).toBe('Supply chain');
    expect(categoryLabel('itsm')).toBe('IT service management');
    expect(categoryLabel('finance')).toBe('Finance');
  });

  it('groups skills by category, both in name order', () => {
    const groups = groupByCategory(skillSummaries);
    expect(groups.map((g) => g.label)).toEqual(['Core', 'Finance', 'Research']);
    expect(groups[0]?.skills.map((s) => s.id)).toEqual(['core-evidence-and-sources']);
  });

  it('matches every word of a search in the name, category or keywords', () => {
    const [evidence, sources, valuation] = skillSummaries as [
      (typeof skillSummaries)[number],
      (typeof skillSummaries)[number],
      (typeof skillSummaries)[number],
    ];
    expect(skillMatches(valuation, 'DCF finance')).toBe(true);
    expect(skillMatches(valuation, 'dcf research')).toBe(false);
    expect(skillMatches(sources, 'bias')).toBe(true);
    expect(skillMatches(evidence, '  ')).toBe(true);
  });
});
