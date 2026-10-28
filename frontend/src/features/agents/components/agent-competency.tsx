import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ClipboardCheckIcon } from 'lucide-react';
import { useId } from 'react';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';
import { problemMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { Agent, AgentCard, AgentEvaluation } from '@/schemas/agent';

import { evaluateAgent } from '../api';
import { useTemplatesById } from '../departments';

function Score({ evaluation }: { evaluation: AgentEvaluation }) {
  const good = evaluation.total > 0 && evaluation.score * 2 >= evaluation.total;
  return (
    <>
      <p className="text-sm">
        <span className={cn('font-medium tabular-nums', !good && 'text-destructive')}>
          {evaluation.score} / {evaluation.total}
        </span>
        <span className="text-muted-foreground">
          {' '}
          competency questions · {evaluation.model} · {formatRelative(evaluation.evaluated_at)}
        </span>
      </p>
      <ul className="space-y-1 text-xs" aria-label="Competency questions">
        {evaluation.questions.map((q) => (
          <li key={q.question} className="flex gap-2">
            <span
              className={cn(
                'shrink-0 font-medium',
                q.passed ? 'text-[var(--status-succeeded)]' : 'text-destructive',
              )}
              aria-label={q.passed ? 'passed' : 'failed'}
            >
              {q.passed ? '✓' : '✗'}
            </span>
            <span className="min-w-0">
              <span>{q.question}</span>
              {q.answer && (
                <span className="block truncate text-muted-foreground" title={q.answer}>
                  {q.answer}
                </span>
              )}
            </span>
          </li>
        ))}
      </ul>
    </>
  );
}

interface AgentCompetencyProps {
  agent: Agent;
  /** The score on its card; `null` until its questions were asked. */
  evaluation: AgentEvaluation | null;
  /** Whether the viewer may ask the questions again (members and above, not guests). */
  member: boolean;
}

/**
 * What an agent made from a role template is measured against: the certification outlines its
 * template is benchmarked on, and its score on the template's competency questions, which
 * members ask again from here. Nothing for an agent made by hand.
 */
export function AgentCompetency({ agent, evaluation, member }: AgentCompetencyProps) {
  const queryClient = useQueryClient();
  const workspaceId = useWorkspaceId();
  const templates = useTemplatesById();
  const template = agent.template_id ? templates?.get(agent.template_id) : undefined;
  const benchmarksId = useId();
  const competencyId = useId();
  const check = useMutation({
    mutationFn: () => evaluateAgent(agent.id),
    meta: { errorToast: false },
    onSuccess: (next) => {
      queryClient.setQueryData<AgentCard>(qk.agents.card(agent.id), (card) =>
        card ? { ...card, evaluation: next } : card,
      );
      void queryClient.invalidateQueries({ queryKey: qk.agents.card(agent.id) });
      void queryClient.invalidateQueries({ queryKey: qk.agents.list(workspaceId) });
      toast.success(`${agent.name} scored ${next.score} / ${next.total} on its questions`);
    },
  });
  if (!agent.template_id) return null;
  const questions = template?.questions.length ?? evaluation?.total;
  return (
    <>
      {template && template.credentials.length > 0 && (
        <section aria-labelledby={benchmarksId} className="space-y-1.5">
          <h3 id={benchmarksId} className="text-xs font-medium text-muted-foreground">
            Benchmarked against
          </h3>
          <ul className="list-disc space-y-0.5 pl-4 text-xs">
            {template.credentials.map((credential) => (
              <li key={credential}>{credential}</li>
            ))}
          </ul>
          <p className="text-xs text-muted-foreground">
            Public certification outlines its knowledge is measured against; the agent holds none of
            these credentials.
          </p>
        </section>
      )}
      <section aria-labelledby={competencyId} className="space-y-1.5">
        <div className="flex items-center justify-between gap-2">
          <h3 id={competencyId} className="text-xs font-medium text-muted-foreground">
            Competency
          </h3>
          {member && (
            <AnimatedButton
              size="xs"
              variant="outline"
              loading={check.isPending}
              loadingText="Asking…"
              onClick={() => check.mutate()}
            >
              <ClipboardCheckIcon />
              Run competency check
            </AnimatedButton>
          )}
        </div>
        {check.isPending && (
          <p role="status" className="text-xs text-muted-foreground">
            Asking its {questions ? `${questions} ` : ''}questions again with the workspace’s model;
            this can take a minute.
          </p>
        )}
        {check.error && (
          <p role="alert" className="text-xs text-destructive">
            {problemMessage(check.error)}
          </p>
        )}
        {evaluation ? (
          <Score evaluation={evaluation} />
        ) : (
          <p className="text-xs text-muted-foreground">
            Not checked yet
            {questions ? `: the check asks its template’s ${questions} questions.` : '.'}
          </p>
        )}
      </section>
    </>
  );
}
