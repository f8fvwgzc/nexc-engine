import { useQuery } from '@tanstack/react-query';
import { LibraryBigIcon, SearchIcon } from 'lucide-react';
import { useEffect, useId, useRef, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { ScrollRegion } from '@/components/custom-ui/scroll-region';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group';
import { Skeleton } from '@/components/ui/skeleton';
import { agentsQuery } from '@/features/agents/api';
import { skillsQuery } from '@/features/skills/api';
import {
  categoryLabel,
  groupByCategory,
  skillMatches,
  skillTokens,
} from '@/features/skills/categories';
import { SkillDetail } from '@/features/skills/components/skill-detail';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';
import { formatTokens } from '@/lib/format';
import { cn } from '@/lib/utils';
import type { SkillSummary } from '@/schemas/skill';

/** The agents of the workspace that have the skill, each a link to its card. */
function GivenTo({ skillId }: { skillId: string }) {
  const workspaceId = useWorkspaceId();
  const { data: agents } = useQuery({
    ...agentsQuery(workspaceId),
    enabled: workspaceId !== undefined,
  });
  if (!agents) return null;
  const holders = agents.filter((a) => a.skills.includes(skillId));
  return (
    <p className="text-xs text-muted-foreground">
      {holders.length === 0 ? (
        'No agent of this workspace has it yet.'
      ) : (
        <>
          Given to{' '}
          {holders.map((agent, i) => (
            <span key={agent.id}>
              {i > 0 && ', '}
              <Link
                to={`/app/agents?agent=${agent.id}`}
                className="text-foreground underline-offset-4 hover:underline"
              >
                {agent.name}
              </Link>
            </span>
          ))}
          .
        </>
      )}
    </p>
  );
}

function SkillPane({ skill }: { skill: SkillSummary }) {
  const titleId = useId();
  return (
    <section aria-labelledby={titleId} className="min-w-0 space-y-4">
      <div className="space-y-1.5 border-b pb-3">
        <h2 id={titleId} className="text-base font-semibold tracking-tight">
          {skill.name}
        </h2>
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          <Badge variant="outline">{categoryLabel(skill.category)}</Badge>
          <code className="font-mono">{skill.id}</code>
          <span>· about {formatTokens(skillTokens(skill.chars))} tokens in a prompt</span>
        </p>
        <GivenTo skillId={skill.id} />
      </div>
      <SkillDetail key={skill.id} skillId={skill.id} summary={skill} />
    </section>
  );
}

function CategoryGroup({
  label,
  skills,
  selectedId,
  onChoose,
}: {
  label: string;
  skills: SkillSummary[];
  selectedId: string | undefined;
  onChoose: (skillId: string) => void;
}) {
  const labelId = useId();
  return (
    <div role="group" aria-labelledby={labelId} className="py-1">
      <p
        id={labelId}
        className="flex items-baseline justify-between px-2 pb-0.5 text-[11px] font-medium text-muted-foreground"
      >
        {label}
        <span className="tabular-nums" aria-hidden>
          {skills.length}
        </span>
      </p>
      <ul>
        {skills.map((skill) => {
          const current = skill.id === selectedId;
          return (
            <li key={skill.id}>
              <button
                type="button"
                onClick={() => onChoose(skill.id)}
                aria-current={current || undefined}
                title={skill.description}
                className={cn(
                  'block w-full truncate rounded-md px-2 py-1 text-left text-[13px] outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50',
                  current && 'bg-muted font-medium',
                )}
              >
                {skill.name}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function LibrarySkeleton() {
  return (
    <div
      className="grid gap-6 lg:grid-cols-[18rem_minmax(0,1fr)]"
      role="status"
      aria-busy
      aria-label="Loading"
    >
      <div className="space-y-2">
        <Skeleton className="h-8 w-full" />
        {Array.from({ length: 8 }, (_, i) => (
          <Skeleton key={i} className="h-6 w-full" />
        ))}
      </div>
      <div className="space-y-3">
        <Skeleton className="h-6 w-56" />
        <Skeleton className="h-4 w-80 max-w-full" />
        <Skeleton className="h-48 w-full" />
      </div>
    </div>
  );
}

/**
 * The skill library: every skill by category with a search, and the chosen one's description,
 * keywords, working knowledge and sources. `?skill=` names the chosen one, so it can be linked.
 */
function Library() {
  const { data: skills, isPending, error, refetch } = useQuery(skillsQuery());
  const [params, setParams] = useSearchParams();
  const [search, setSearch] = useState('');
  const listId = useId();
  const listRef = useRef<HTMLDivElement>(null);
  // Opened on a link to one skill: bring it into view in the list.
  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>('[aria-current="true"]')
      ?.scrollIntoView?.({ block: 'nearest' });
  }, [isPending]);
  if (isPending) return <LibrarySkeleton />;
  if (error) {
    return (
      <ErrorState title="Couldn’t load the skills" error={error} onRetry={() => void refetch()} />
    );
  }
  if (skills.length === 0) {
    return (
      <EmptyState
        icon={LibraryBigIcon}
        title="No skills in the library"
        description="The library ships with the server: this build has none."
      />
    );
  }
  const groups = groupByCategory(skills.filter((s) => skillMatches(s, search)));
  const asked = params.get('skill');
  const selected = asked ? skills.find((s) => s.id === asked) : (groups[0]?.skills[0] ?? skills[0]);
  const choose = (id: string) =>
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.set('skill', id);
        return next;
      },
      { replace: true },
    );
  return (
    <div className="grid gap-6 lg:grid-cols-[18rem_minmax(0,1fr)]">
      <div className="min-w-0 space-y-2">
        <InputGroup>
          <InputGroupAddon>
            <SearchIcon aria-hidden />
          </InputGroupAddon>
          <InputGroupInput
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search skills"
            aria-label="Search skills"
            aria-controls={listId}
          />
        </InputGroup>
        <ScrollRegion
          label="Skills by category"
          className="max-h-80 overflow-y-auto rounded-lg border p-1 lg:max-h-[calc(100svh-14rem)]"
        >
          <div id={listId} ref={listRef}>
            {groups.length === 0 ? (
              <p className="p-3 text-xs text-muted-foreground">
                No skill matches “{search.trim()}”.
              </p>
            ) : (
              groups.map((group) => (
                <CategoryGroup
                  key={group.category}
                  label={group.label}
                  skills={group.skills}
                  selectedId={selected?.id}
                  onChoose={choose}
                />
              ))
            )}
          </div>
        </ScrollRegion>
      </div>
      {selected ? (
        <SkillPane skill={selected} />
      ) : (
        <EmptyState
          compact
          icon={LibraryBigIcon}
          title="No such skill"
          description={`The library has no skill “${asked ?? ''}”: pick one from the list.`}
        />
      )}
    </div>
  );
}

export default function SkillsPage() {
  const { data: skills } = useQuery(skillsQuery());
  const categories = new Set(skills?.map((s) => s.category)).size;
  return (
    <PageContainer>
      <Seo title="Skills" noIndex />
      <PageHeader
        title="Skills"
        description={`The working knowledge agents are given${
          skills ? `: ${skills.length} skills in ${categories} fields` : ''
        }, each written from public sources. An agent's skills are added to its prompt for each step, the most relevant first.`}
      />
      <Library />
    </PageContainer>
  );
}
