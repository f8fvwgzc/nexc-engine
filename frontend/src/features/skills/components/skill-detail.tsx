import { useQuery } from '@tanstack/react-query';
import { ExternalLinkIcon } from 'lucide-react';
import { useId } from 'react';

import { ErrorState } from '@/components/custom-ui/error-state';
import { Markdown } from '@/components/custom-ui/markdown';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import type { SkillSource, SkillSummary } from '@/schemas/skill';

import { skillQuery } from '../api';

/** The host of a source, to say where a link goes; `null` for anything but an http(s) URL. */
function sourceHost(url: string): string | null {
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'https:' || parsed.protocol === 'http:' ? parsed.hostname : null;
  } catch {
    return null;
  }
}

function Sources({ sources }: { sources: SkillSource[] }) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="space-y-1.5">
      <h3 id={headingId} className="text-xs font-medium text-muted-foreground">
        Sources
      </h3>
      <ul className="space-y-1 text-[13px]">
        {sources.map((source) => {
          const host = sourceHost(source.url);
          return (
            <li key={source.url} className="min-w-0">
              {host ? (
                <a
                  href={source.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex max-w-full items-center gap-1 text-brand underline-offset-4 hover:underline focus-visible:underline"
                >
                  <span className="truncate">{source.title}</span>
                  <ExternalLinkIcon className="size-3 shrink-0" aria-hidden />
                  <span className="sr-only"> (opens in a new tab)</span>
                </a>
              ) : (
                <span>{source.title}</span>
              )}
              {host && <span className="text-xs text-muted-foreground"> · {host}</span>}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

interface SkillDetailProps {
  skillId: string;
  /** What the library list already says about it: shown while the knowledge loads. */
  summary?: SkillSummary;
}

/**
 * A skill: what it is for, the terms that bring it into a step, its working knowledge as the
 * agents read it, and the public pages it was written from (each opens in a new tab).
 */
export function SkillDetail({ skillId, summary }: SkillDetailProps) {
  const { data: skill, error, isPending, refetch } = useQuery(skillQuery(skillId));
  const shown = skill ?? summary;
  const knowledgeId = useId();
  return (
    <div className="space-y-4">
      {shown && <p className="text-sm text-pretty">{shown.description}</p>}
      {shown && shown.keywords.length > 0 && (
        <ul className="flex flex-wrap gap-1" aria-label="Keywords">
          {shown.keywords.map((keyword) => (
            <li key={keyword}>
              <Badge variant="secondary" className="font-normal">
                {keyword}
              </Badge>
            </li>
          ))}
        </ul>
      )}
      <section aria-labelledby={knowledgeId} className="space-y-1.5">
        <h3 id={knowledgeId} className="text-xs font-medium text-muted-foreground">
          Working knowledge
        </h3>
        {isPending ? (
          <div className="space-y-2" aria-busy>
            <Skeleton className="h-4 w-1/2" />
            <Skeleton className="h-24 w-full" />
          </div>
        ) : error ? (
          <ErrorState
            compact
            title="Couldn’t load the skill"
            error={error}
            onRetry={() => void refetch()}
          />
        ) : (
          <Markdown className="rounded-lg border bg-muted/20 p-3">{skill.body}</Markdown>
        )}
      </section>
      {shown && shown.sources.length > 0 && <Sources sources={shown.sources} />}
    </div>
  );
}
