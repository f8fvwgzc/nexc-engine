import { matchesWords } from '@/lib/utils';
import type { SkillSummary } from '@/schemas/skill';

/** Category ids whose name is not their id with a capital (acronyms, short forms). */
const LABELS: Record<string, string> = {
  appsec: 'Application security',
  devops: 'DevOps',
  hr: 'HR',
  itsm: 'IT service management',
  ml: 'Machine learning',
  qa: 'Quality assurance',
};

/** `supply-chain` → `Supply chain`. */
export function categoryLabel(category: string): string {
  const known = LABELS[category];
  if (known) return known;
  const words = category.replace(/[-_]+/g, ' ').trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : category;
}

/** Whether a skill matches every word of a search, in its name, id, category, text or keywords. */
export function skillMatches(skill: SkillSummary, search: string): boolean {
  return matchesWords(
    [
      skill.name,
      skill.id,
      skill.category,
      categoryLabel(skill.category),
      skill.description,
      ...skill.keywords,
    ],
    search,
  );
}

/** Skills in groups by category, the groups by name and the skills by name within each. */
export function groupByCategory(
  skills: readonly SkillSummary[],
): { category: string; label: string; skills: SkillSummary[] }[] {
  const groups = new Map<string, SkillSummary[]>();
  for (const skill of skills) {
    const group = groups.get(skill.category);
    if (group) group.push(skill);
    else groups.set(skill.category, [skill]);
  }
  return [...groups.entries()]
    .map(([category, list]) => ({
      category,
      label: categoryLabel(category),
      skills: [...list].sort((a, b) => a.name.localeCompare(b.name)),
    }))
    .sort((a, b) => a.label.localeCompare(b.label));
}

/** Roughly what a skill costs in a prompt: four characters to a token. */
export function skillTokens(chars: number): number {
  return Math.ceil(chars / 4);
}
