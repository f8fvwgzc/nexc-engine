import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { SearchIcon } from 'lucide-react';
import { RadioGroup as RadioGroupPrimitive } from 'radix-ui';
import { useId, useState } from 'react';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { ScrollRegion } from '@/components/custom-ui/scroll-region';
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
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group';
import { Skeleton } from '@/components/ui/skeleton';
import { Switch } from '@/components/ui/switch';
import { summarizeCapabilities } from '@/features/agents/capabilities';
import { groupByDepartment } from '@/features/agents/departments';
import { skillsQuery } from '@/features/skills/api';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { Agent } from '@/schemas/agent';
import type { AgentTemplate } from '@/schemas/secops';

import { agentTemplatesQuery, createAgentFromTemplate } from '../api';
import { templateMatches } from '../templates';

const count = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** What the chosen template gives an agent: its department, benchmarks, skills, scopes, questions. */
function TemplateDetail({ template }: { template: AgentTemplate }) {
  const { data: skills } = useQuery(skillsQuery());
  const names = new Map(skills?.map((s) => [s.id, s.name]));
  return (
    <div className="min-w-0 space-y-3 rounded-lg border bg-muted/30 p-3 text-xs">
      <div>
        <p className="text-sm font-medium">{template.name}</p>
        <p className="text-muted-foreground">
          {template.title} · {template.department}
        </p>
      </div>
      <p>{template.description}</p>
      {template.credentials.length > 0 && (
        <div className="space-y-1">
          <p className="font-medium">Benchmarked against</p>
          <ul className="list-disc space-y-0.5 pl-4">
            {template.credentials.map((credential) => (
              <li key={credential}>{credential}</li>
            ))}
          </ul>
          <p className="text-muted-foreground">
            Public certification outlines its knowledge is measured against; the agent holds none of
            these credentials.
          </p>
        </div>
      )}
      <p>
        <span className="font-medium">
          {count(template.skills.length, 'skill')} ·{' '}
          {count(template.questions.length, 'competency question')}
        </span>
        {template.skills.length > 0 && (
          <span className="block text-muted-foreground">
            {template.skills.map((id) => names.get(id) ?? id).join(' · ')}
          </span>
        )}
      </p>
      <ul className="space-y-0.5">
        {summarizeCapabilities(template.capabilities).map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      {template.questions.length > 0 && (
        <div className="space-y-0.5">
          <p className="font-medium">Asks</p>
          <ol className="list-decimal space-y-0.5 pl-4 text-muted-foreground">
            {template.questions.map((q) => (
              <li key={q.question}>{q.question}</li>
            ))}
          </ol>
        </div>
      )}
    </div>
  );
}

interface AgentTemplateDialogProps {
  workspaceId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated?: (agent: Agent) => void;
}

/**
 * Makes an agent from one of the role templates: find the role by department or by search, name
 * it, and choose whether its competency questions are asked now (the score lands on its card).
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
  const [search, setSearch] = useState('');
  const [name, setName] = useState('');
  const [evaluate, setEvaluate] = useState(true);
  const nameId = useId();
  const listId = useId();
  const groups = groupByDepartment(
    (templates ?? []).filter((t) => templateMatches(t, search)),
    (t) => t.department,
  );
  const shown = groups.flatMap((g) => g.items);
  const template =
    templates?.find((t) => t.id === picked) ?? shown[0] ?? groups[0]?.items[0] ?? templates?.[0];
  const departments = new Set(templates?.map((t) => t.department)).size;
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
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>New agent from a template</DialogTitle>
          <DialogDescription>
            {templates
              ? `${count(templates.length, 'role')} in ${count(departments, 'department')}, `
              : 'Roles '}
            each with its scopes, its own policy, its skills and competency questions; the agent
            works on day one and its card shows how it answered.
          </DialogDescription>
        </DialogHeader>
        {isPending ? (
          <Skeleton className="h-40 rounded-lg" />
        ) : error ? (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(error)}
          </p>
        ) : (
          <div className="min-w-0 space-y-4">
            <InputGroup>
              <InputGroupAddon>
                <SearchIcon aria-hidden />
              </InputGroupAddon>
              <InputGroupInput
                type="search"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search by role, department or certification"
                aria-label="Search templates"
                aria-controls={listId}
              />
            </InputGroup>
            <div className="grid min-w-0 gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
              <ScrollRegion
                label="Templates"
                className="max-h-80 min-w-0 overflow-y-auto rounded-lg border p-1"
              >
                {shown.length === 0 ? (
                  <p id={listId} className="p-3 text-xs text-muted-foreground">
                    No template matches “{search.trim()}”.
                  </p>
                ) : (
                  <RadioGroupPrimitive.Root
                    id={listId}
                    value={template?.id ?? ''}
                    onValueChange={setPicked}
                    aria-label="Template"
                    className="space-y-2"
                  >
                    {groups.map((group) => (
                      <div key={group.department} role="group" aria-label={group.department}>
                        <p
                          aria-hidden
                          className="px-2 pt-1 pb-0.5 text-[11px] font-medium text-muted-foreground"
                        >
                          {group.department}
                        </p>
                        {group.items.map((t) => (
                          <RadioGroupPrimitive.Item
                            key={t.id}
                            value={t.id}
                            className="block w-full rounded-md border border-transparent px-2 py-1.5 text-left text-xs outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50 data-[state=checked]:border-brand data-[state=checked]:bg-brand/5"
                          >
                            <span className="block truncate text-[13px] font-medium">{t.name}</span>
                            <span className="block truncate text-muted-foreground">
                              {count(t.skills.length, 'skill')} ·{' '}
                              {count(t.questions.length, 'question')}
                              {t.credentials[0] ? ` · ${t.credentials[0]}` : ''}
                            </span>
                          </RadioGroupPrimitive.Item>
                        ))}
                      </div>
                    ))}
                  </RadioGroupPrimitive.Root>
                )}
              </ScrollRegion>
              {template && <TemplateDetail template={template} />}
            </div>
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
