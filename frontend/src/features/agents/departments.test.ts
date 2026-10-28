import { describe, expect, it } from 'vitest';

import { agent, agentTemplate } from '@/test/fixtures';

import { CORE_TEAM, CUSTOM, agentDepartment, groupByDepartment } from './departments';

const templates = new Map([
  [agentTemplate.id, agentTemplate],
  ['analyst', { ...agentTemplate, id: 'analyst', department: 'Finance & Risk' }],
]);

describe('agentDepartment', () => {
  it('takes the department of the template the agent was made from', () => {
    expect(agentDepartment({ ...agent, template_id: 'analyst' }, templates)).toBe('Finance & Risk');
    expect(agentDepartment(agent, templates)).toBe(CORE_TEAM);
  });

  it('puts a core role made by hand in the core team, anything else in Custom', () => {
    expect(agentDepartment({ ...agent, template_id: null, role: 'planner' }, templates)).toBe(
      CORE_TEAM,
    );
    expect(agentDepartment({ ...agent, template_id: null, role: 'pilot' }, templates)).toBe(CUSTOM);
    // The template is gone from the catalogue.
    expect(agentDepartment({ ...agent, template_id: 'retired' }, templates)).toBe(CUSTOM);
  });

  it('does not guess while the templates are not known yet', () => {
    expect(agentDepartment(agent, undefined)).toBeNull();
    expect(agentDepartment({ ...agent, template_id: null }, undefined)).toBe(CORE_TEAM);
  });
});

describe('groupByDepartment', () => {
  it('orders the departments as the catalogue does, others by name, Custom last', () => {
    const groups = groupByDepartment(
      ['Custom', 'Zoology', 'Marketing & Sales', 'Core team', 'Finance & Risk', 'Archery'],
      (d) => d,
    );
    expect(groups.map((g) => g.department)).toEqual([
      'Core team',
      'Finance & Risk',
      'Marketing & Sales',
      'Archery',
      'Zoology',
      'Custom',
    ]);
  });

  it('keeps the order of the items within a department', () => {
    const groups = groupByDepartment(
      [
        { name: 'b', d: 'Finance & Risk' },
        { name: 'a', d: 'Core team' },
        { name: 'c', d: 'Finance & Risk' },
      ],
      (item) => item.d,
    );
    expect(groups.map((g) => g.items.map((i) => i.name))).toEqual([['a'], ['b', 'c']]);
  });
});
