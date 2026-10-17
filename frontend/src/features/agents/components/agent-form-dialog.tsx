import { zodResolver } from '@hookform/resolvers/zod';
import { useQuery } from '@tanstack/react-query';
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
import { teamsQuery } from '@/features/workspaces/api';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import {
  DEFAULT_CAPABILITIES,
  agentInputSchema,
  type Agent,
  type AgentInput,
} from '@/schemas/agent';

import { managerCandidates } from '../agent-tree';
import { useSaveAgent } from '../hooks/use-agent-mutations';
import { CapabilitiesEditor } from './capabilities-editor';

const NONE = '__none__';

const EMPTY: AgentInput = {
  name: '',
  role: '',
  title: '',
  model: '',
  system_prompt: '',
  reports_to: null,
  budget_tokens: 200_000,
  runtime: 'python',
  purpose: '',
  team_id: null,
  capabilities: DEFAULT_CAPABILITIES,
};

function toInput(agent: Agent): AgentInput {
  const {
    name,
    role,
    title,
    model,
    system_prompt,
    reports_to,
    budget_tokens,
    runtime,
    purpose,
    team_id,
    capabilities,
  } = agent;
  return {
    name,
    role,
    title,
    model,
    system_prompt,
    reports_to,
    budget_tokens,
    runtime,
    purpose,
    team_id,
    capabilities,
  };
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
  'purpose',
  'team_id',
  'capabilities',
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
  const workspaceId = useWorkspaceId();
  const { data: teams = [] } = useQuery({
    ...teamsQuery(workspaceId ?? ''),
    enabled: open && workspaceId !== undefined,
  });
  const form = useForm<AgentInput>({
    resolver: zodResolver(agentInputSchema),
    defaultValues: EMPTY,
  });

  useEffect(() => {
    if (open) form.reset(agent ? toInput(agent) : EMPTY);
  }, [open, agent, form]);

  const managers = [
    { value: NONE, label: 'Nobody (top level)' },
    ...managerCandidates(agents, agent?.id ?? null).map((a) => ({
      value: a.id,
      label: `${a.name} · ${a.role}`,
    })),
  ];
  const teamOptions = [
    { value: NONE, label: 'No team' },
    ...teams.map((t) => ({ value: t.id, label: `${t.name} (${t.key})` })),
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
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
        <form noValidate onSubmit={(e) => void submit(e)} className="space-y-6">
          <DialogHeader>
            <DialogTitle>{agent ? `Edit ${agent.name}` : 'New agent'}</DialogTitle>
            <DialogDescription>
              Agents execute nodes whose <code>agent_role</code> matches their role, within a token
              budget and the capabilities below. Every change to the capabilities raises the
              agent&apos;s policy version.
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
                    value={value ?? NONE}
                    onValueChange={(v) => onChange(v === NONE ? null : v)}
                    options={managers}
                  />
                )}
              </FormField>
              <FormField control={form.control} name="team_id" label="Team">
                {({ value, onChange, ...field }) => (
                  <OptionSelect
                    {...field}
                    value={value ?? NONE}
                    onValueChange={(v) => onChange(v === NONE ? null : v)}
                    options={teamOptions}
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
            <FormField
              control={form.control}
              name="purpose"
              label="Purpose"
              description="What the agent is for, as shown on its card."
            >
              {(field) => (
                <Textarea
                  {...field}
                  rows={2}
                  placeholder="Gathers and weighs evidence for the other agents to build on."
                />
              )}
            </FormField>
            <FormField control={form.control} name="system_prompt" label="System prompt">
              {(field) => (
                <Textarea
                  {...field}
                  rows={5}
                  className="font-mono text-[13px]"
                  placeholder="You are a meticulous researcher…"
                />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="capabilities"
              label="Capabilities"
              description="What the agent may do; its per-run token carries a copy, and the runtime and the backend both enforce it."
            >
              {({ value, onChange }) => (
                <CapabilitiesEditor
                  value={value}
                  onChange={onChange}
                  className="rounded-lg border p-3"
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
