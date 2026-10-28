import { useQuery } from '@tanstack/react-query';
import { useId, useState } from 'react';

import { skillsQuery } from '@/features/skills/api';
import { SkillDialog } from '@/features/skills/components/skill-dialog';
import { AGENT_SKILLS_MAX } from '@/schemas/agent';

/**
 * The agent's skills as chips, most relevant first, named from the cached library; a chip opens
 * the skill's working knowledge and sources.
 */
export function AgentSkills({ skillIds }: { skillIds: string[] }) {
  const { data: skills } = useQuery(skillsQuery());
  const [open, setOpen] = useState<string | null>(null);
  const headingId = useId();
  const byId = new Map(skills?.map((s) => [s.id, s]));
  return (
    <section aria-labelledby={headingId} className="space-y-1.5">
      <h3 id={headingId} className="text-xs font-medium text-muted-foreground">
        Skills{' '}
        <span className="tabular-nums">
          · {skillIds.length} of {AGENT_SKILLS_MAX}
        </span>
      </h3>
      {skillIds.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          No skills yet: edit the agent to give it working knowledge from the library.
        </p>
      ) : (
        <ul className="flex flex-wrap gap-1.5" aria-label="Skills">
          {skillIds.map((id) => {
            const skill = byId.get(id);
            return (
              <li key={id}>
                <button
                  type="button"
                  onClick={() => setOpen(id)}
                  title={skill?.description}
                  className="inline-flex h-6 max-w-56 items-center rounded-full border bg-muted/40 px-2.5 text-xs outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
                >
                  <span className="truncate">{skill?.name ?? id}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
      <SkillDialog skillId={open} onOpenChange={(next) => !next && setOpen(null)} />
    </section>
  );
}
