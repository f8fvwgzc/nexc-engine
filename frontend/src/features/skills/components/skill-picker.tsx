import { useQuery } from '@tanstack/react-query';
import { PlusIcon, XIcon } from 'lucide-react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { AGENT_SKILLS_MAX } from '@/schemas/agent';

import { skillsQuery } from '../api';
import { groupByCategory } from '../categories';

interface SkillPickerProps {
  /** Ids of the chosen skills, most relevant first. */
  value: string[];
  onChange: (skillIds: string[]) => void;
  id?: string;
  'aria-invalid'?: boolean;
  'aria-describedby'?: string;
}

/**
 * Chooses an agent's skills from the library: the chosen ones as chips in their order (new ones
 * go last), and a searchable list by category to tick skills on and off, up to the limit.
 */
export function SkillPicker({ value, onChange, id, ...aria }: SkillPickerProps) {
  const { data: skills, isPending, error } = useQuery(skillsQuery());
  const [open, setOpen] = useState(false);
  const byId = new Map(skills?.map((s) => [s.id, s]));
  const chosen = new Set(value);
  const full = value.length >= AGENT_SKILLS_MAX;
  const toggle = (skillId: string) => {
    if (chosen.has(skillId)) onChange(value.filter((v) => v !== skillId));
    else if (!full) onChange([...value, skillId]);
  };

  return (
    <div className="space-y-2">
      {value.length > 0 && (
        <ul className="flex flex-wrap gap-1.5" aria-label="Chosen skills">
          {value.map((skillId) => {
            const name = byId.get(skillId)?.name ?? skillId;
            return (
              <li
                key={skillId}
                className="inline-flex h-6 max-w-64 items-center gap-0.5 rounded-full border bg-muted/40 pr-0.5 pl-2.5 text-xs"
              >
                <span className="truncate">{name}</span>
                <button
                  type="button"
                  aria-label={`Remove ${name}`}
                  onClick={() => onChange(value.filter((v) => v !== skillId))}
                  className="inline-flex size-5 shrink-0 items-center justify-center rounded-full text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
                >
                  <XIcon className="size-3" aria-hidden />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {/* Modal, so its list scrolls with the wheel inside the agent dialog. */}
      <Popover modal open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="outline"
            size="sm"
            id={id}
            {...aria}
            aria-label={`Add skills, ${value.length} of ${AGENT_SKILLS_MAX} chosen`}
          >
            <PlusIcon />
            Add skills
            <span className="text-muted-foreground tabular-nums">
              {value.length} / {AGENT_SKILLS_MAX}
            </span>
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-[min(26rem,calc(100vw-2rem))] gap-0 p-0">
          {isPending ? (
            <div className="space-y-2 p-3" aria-busy>
              <Skeleton className="h-7 w-full" />
              <Skeleton className="h-24 w-full" />
            </div>
          ) : error ? (
            <p role="alert" className="p-3 text-xs text-destructive">
              {errorMessage(error)}
            </p>
          ) : (
            <Command loop label="Search skills" className="rounded-lg!">
              <CommandInput placeholder="Search skills" />
              <CommandList label="Skills by category">
                <CommandEmpty>No skill matches.</CommandEmpty>
                {groupByCategory(skills).map((group) => (
                  <CommandGroup key={group.category} heading={group.label}>
                    {group.skills.map((skill) => {
                      const selected = chosen.has(skill.id);
                      return (
                        <CommandItem
                          key={skill.id}
                          value={skill.id}
                          keywords={[skill.name, group.label, ...skill.keywords]}
                          data-checked={selected}
                          aria-checked={selected}
                          disabled={!selected && full}
                          onSelect={() => toggle(skill.id)}
                        >
                          <span className="min-w-0">
                            <span className="block truncate">{skill.name}</span>
                            <span className="block truncate text-xs text-muted-foreground">
                              {skill.description}
                            </span>
                          </span>
                        </CommandItem>
                      );
                    })}
                  </CommandGroup>
                ))}
              </CommandList>
            </Command>
          )}
          <p className="border-t px-3 py-2 text-xs text-muted-foreground" aria-live="polite">
            {value.length} of {AGENT_SKILLS_MAX} chosen
            {full ? ': remove one to add another.' : '.'}
          </p>
        </PopoverContent>
      </Popover>
    </div>
  );
}
