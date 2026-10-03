import { zodResolver } from '@hookform/resolvers/zod';
import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { FormField } from '@/components/custom-ui/form-field';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import { agentInputSchema, type Agent, type AgentInput } from '@/schemas/agent';

import { managerCandidates } from '../agent-tree';
import { useSaveAgent } from '../hooks/use-agent-mutations';

const NO_MANAGER = '__none__';

const EMPTY: AgentInput = {
  name: '',
  role: '',
  title: '',
  model: '',
  system_prompt: '',
  reports_to: null,
  budget_tokens: 200_000,
  runtime: 'python',
};

function toInput(agent: Agent): AgentInput {
  const { name, role, title, model, system_prompt, reports_to, budget_tokens, runtime } = agent;
  return { name, role, title, model, system_prompt, reports_to, budget_tokens, runtime };
}

const FIELDS = [
  'name',
  'role',
  'title',
  'model',
  'system_prompt',
  'reports_to',
  'budget_tokens',
  'runtime',
] as const;

interface AgentFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** null → create a new agent. */
  agent: Agent | null;
  agents: Agent[];
}

export function AgentFormDialog({ open, onOpenChange, agent, agents }: AgentFormDialogProps) {
  const saveAgent = useSaveAgent();
  const form = useForm<AgentInput>({
    resolver: zodResolver(agentInputSchema),
    defaultValues: EMPTY,
  });

  useEffect(() => {
    if (open) form.reset(agent ? toInput(agent) : EMPTY);
  }, [open, agent, form]);

  const managers = [
    { value: NO_MANAGER, label: 'Nobody (top level)' },
    ...managerCandidates(agents, agent?.id ?? null).map((a) => ({
      value: a.id,
      label: `${a.name} · ${a.role}`,
    })),
  ];

  const submit = form.handleSubmit((body) =>
    saveAgent.mutate(
      { id: agent?.id ?? null, body },
      {
        onSuccess: (saved) => {
          toast.success(agent ? 'Agent updated' : `Agent “${saved.name}” created`);
          onOpenChange(false);
        },
        onError: (error) => {
          if (!applyProblemToForm(error, form.setError, FIELDS)) toast.error(errorMessage(error));
        },
      },
    ),
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-xl">
        <form noValidate onSubmit={(e) => void submit(e)} className="space-y-6">
          <DialogHeader>
            <DialogTitle>{agent ? `Edit ${agent.name}` : 'New agent'}</DialogTitle>
            <DialogDescription>
              Agents execute nodes whose <code>agent_role</code> matches their role, within a token
              budget.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup className="gap-4">
            <div className="grid gap-4 sm:grid-cols-2">
              <FormField control={form.control} name="name" label="Name">
                {(field) => <Input {...field} placeholder="Ada" />}
              </FormField>
              <FormField control={form.control} name="role" label="Role">
                {(field) => <Input {...field} placeholder="researcher" />}
              </FormField>
              <FormField control={form.control} name="title" label="Title">
                {(field) => <Input {...field} placeholder="Head of Research" />}
              </FormField>
              <FormField
                control={form.control}
                name="model"
                label="Model"
                description="Blank = server default."
              >
                {(field) => <Input {...field} placeholder="claude-opus-5" />}
              </FormField>
              <FormField control={form.control} name="reports_to" label="Reports to">
                {({ value, onChange, ...field }) => (
                  <OptionSelect
                    {...field}
                    value={value ?? NO_MANAGER}
                    onValueChange={(v) => onChange(v === NO_MANAGER ? null : v)}
                    options={managers}
                  />
                )}
              </FormField>
              <FormField control={form.control} name="runtime" label="Runtime">
                {({ value, onChange, ...field }) => (
                  <OptionSelect
                    {...field}
                    value={value}
                    onValueChange={onChange}
                    options={[
                      { value: 'python', label: 'Python agent runtime' },
                      { value: 'builtin', label: 'Built-in' },
                    ]}
                  />
                )}
              </FormField>
              <FormField
                control={form.control}
                name="budget_tokens"
                label="Token budget"
                description="0 = no limit."
                className="sm:col-span-2"
              >
                {({ value, onChange, ...field }) => (
                  <Input
                    {...field}
                    type="number"
                    inputMode="numeric"
                    min={0}
                    step={1000}
                    value={Number.isNaN(value) ? '' : value}
                    onChange={(e) => onChange(e.target.valueAsNumber)}
                  />
                )}
              </FormField>
            </div>
            <FormField control={form.control} name="system_prompt" label="System prompt">
              {(field) => (
                <Textarea
                  {...field}
                  rows={6}
                  className="font-mono text-[13px]"
                  placeholder="You are a meticulous researcher…"
                />
              )}
            </FormField>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <AnimatedButton type="submit" loading={saveAgent.isPending} loadingText="Saving…">
              {agent ? 'Save changes' : 'Create agent'}
            </AnimatedButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
