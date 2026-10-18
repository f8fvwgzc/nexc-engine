import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useId, useState } from 'react';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Switch } from '@/components/ui/switch';
import { summarizeCapabilities } from '@/features/agents/capabilities';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { Agent } from '@/schemas/agent';

import { agentTemplatesQuery, createAgentFromTemplate } from '../api';

interface AgentTemplateDialogProps {
  workspaceId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated?: (agent: Agent) => void;
}

/**
 * Makes an agent from one of the role templates: pick the role, name it, and choose whether
 * its competency questions are asked now (the score lands on its card).
 */
export function AgentTemplateDialog({
  workspaceId,
  open,
  onOpenChange,
  onCreated,
}: AgentTemplateDialogProps) {
  const queryClient = useQueryClient();
  const { data: templates, isPending, error } = useQuery(agentTemplatesQuery());
  const [picked, setPicked] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [evaluate, setEvaluate] = useState(true);
  const nameId = useId();
  const template = templates?.find((t) => t.id === picked) ?? templates?.[0];
  const create = useMutation({
    mutationFn: () =>
      createAgentFromTemplate(template?.id ?? '', {
        workspace_id: workspaceId,
        ...(name.trim() ? { name: name.trim() } : {}),
        evaluate,
      }),
    onSuccess: (card) => {
      const score = card.evaluation
        ? `; scored ${card.evaluation.score} / ${card.evaluation.total} on its questions`
        : '';
      toast.success(`${card.agent.name} is ready${score}`);
      void queryClient.invalidateQueries({ queryKey: qk.agents.all });
      void queryClient.invalidateQueries({ queryKey: qk.secops.all });
      void queryClient.invalidateQueries({ queryKey: qk.policies.all });
      queryClient.setQueryData(qk.agents.card(card.agent.id), card);
      onOpenChange(false);
      onCreated?.(card.agent);
    },
    onError: (err) => toast.error(errorMessage(err)),
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>New agent from a template</DialogTitle>
          <DialogDescription>
            A role with its scopes, its own policy and three competency questions; the agent works
            on day one and its card shows how it answered.
          </DialogDescription>
        </DialogHeader>
        {isPending ? (
          <Skeleton className="h-40 rounded-lg" />
        ) : error ? (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(error)}
          </p>
        ) : (
          <div className="space-y-4">
            <div className="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label="Template">
              {templates.map((t) => {
                const selected = t.id === template?.id;
                return (
                  <button
                    key={t.id}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    onClick={() => setPicked(t.id)}
                    className={cn(
                      'rounded-lg border p-3 text-left text-xs transition-colors outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50',
                      selected && 'border-brand bg-brand/5',
                    )}
                  >
                    <p className="text-sm font-medium">{t.name}</p>
                    <p className="text-muted-foreground">{t.title}</p>
                    <p className="mt-1 line-clamp-3 text-muted-foreground">{t.description}</p>
                  </button>
                );
              })}
            </div>
            {template && (
              <ul className="space-y-0.5 rounded-lg border bg-muted/30 p-3 text-xs">
                {summarizeCapabilities(template.capabilities).map((line) => (
                  <li key={line}>{line}</li>
                ))}
                <li className="pt-1 text-muted-foreground">
                  Asks: {template.questions.map((q) => q.question).join(' · ')}
                </li>
              </ul>
            )}
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
              <div className="space-y-1">
                <label htmlFor={nameId} className="text-xs font-medium">
                  Name
                </label>
                <Input
                  id={nameId}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={template?.name ?? 'Name'}
                  maxLength={64}
                />
              </div>
              <label className="flex items-center gap-2 pb-2 text-xs">
                <Switch checked={evaluate} onCheckedChange={setEvaluate} />
                Ask its questions now
              </label>
            </div>
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <AnimatedButton
            loading={create.isPending}
            disabled={!template}
            onClick={() => create.mutate()}
          >
            Create {template?.name ?? 'agent'}
          </AnimatedButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
