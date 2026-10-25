import { matchesWords } from '@/lib/utils';
import type { AgentTemplate } from '@/schemas/secops';

/** Whether a template matches every word of a search, in its names, department or credentials. */
export function templateMatches(template: AgentTemplate, search: string): boolean {
  return matchesWords(
    [
      template.name,
      template.title,
      template.role,
      template.department,
      template.description,
      ...template.credentials,
    ],
    search,
  );
}
