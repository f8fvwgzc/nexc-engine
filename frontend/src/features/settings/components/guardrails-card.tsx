import { useMutation, useQueryClient, useSuspenseQuery } from '@tanstack/react-query';
import { ShieldCheckIcon } from 'lucide-react';
import { useState } from 'react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { guardrailsQuery, saveGuardrails } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';
import { isWorkspaceAdmin, type Guardrails, type Workspace } from '@/schemas/workspace';

const PROVIDERS: { value: Guardrails['allowed_providers'][number]; label: string }[] = [
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'openai_compatible', label: 'OpenAI-compatible' },
  { value: 'claude_code', label: 'Claude Code CLI' },
  { value: 'demo', label: 'Demo' },
];

/** A whole number of tokens, or null when the field is empty (no limit). */
const parseBudget = (text: string): number | null => {
  const digits = text.replace(/\D/g, '');
  return digits ? Number(digits) : null;
};

function GuardrailsForm({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const { data: stored } = useSuspenseQuery(guardrailsQuery(workspace.id));
  const [draft, setDraft] = useState(stored);
  const canEdit = isWorkspaceAdmin(workspace.role);
  const save = useMutation({
    mutationFn: () => saveGuardrails(workspace.id, draft),
    meta: { successMessage: 'Guardrails saved' },
    onSuccess: (saved) => {
      queryClient.setQueryData(qk.workspaces.guardrails(workspace.id), saved);
      setDraft(saved);
    },
  });
  const dirty = JSON.stringify(draft) !== JSON.stringify(stored);
  const toggleProvider = (value: Guardrails['allowed_providers'][number], on: boolean) =>
    setDraft((d) => ({
      ...d,
      allowed_providers: on
        ? [...d.allowed_providers, value]
        : d.allowed_providers.filter((p) => p !== value),
    }));

  return (
    <GlassCard className="space-y-5 p-5 sm:p-6">
      <div className="space-y-1">
        <h2 className="flex items-center gap-2 font-medium">
          <ShieldCheckIcon className="size-4" aria-hidden />
          Guardrails
        </h2>
        <p className="text-sm text-muted-foreground">
          Limits {workspace.name} puts on its use of AI. They are checked before a plan, a run or an
          assistant reply starts.
          {canEdit ? '' : ' Only workspace admins can change them.'}
        </p>
      </div>
      <fieldset disabled={!canEdit} className="space-y-5">
        <div className="grid gap-4 sm:grid-cols-2">
          <label className="space-y-1.5 text-sm">
            <span className="font-medium">Workspace tokens per month</span>
            <Input
              inputMode="numeric"
              value={draft.monthly_token_budget ?? ''}
              placeholder="No limit"
              onChange={(e) =>
                setDraft((d) => ({ ...d, monthly_token_budget: parseBudget(e.target.value) }))
              }
            />
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="font-medium">Tokens per member per month</span>
            <Input
              inputMode="numeric"
              value={draft.member_monthly_token_budget ?? ''}
              placeholder="No limit"
              onChange={(e) =>
                setDraft((d) => ({
                  ...d,
                  member_monthly_token_budget: parseBudget(e.target.value),
                }))
              }
            />
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="font-medium">Most memories kept</span>
            <Input
              inputMode="numeric"
              value={draft.memory_limit ?? ''}
              placeholder="Keep all (at least 100)"
              onChange={(e) =>
                setDraft((d) => ({ ...d, memory_limit: parseBudget(e.target.value) }))
              }
            />
            <span className="block text-xs text-muted-foreground">
              Past it, the least important and least recalled are forgotten first.
            </span>
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="font-medium">Forget memories untouched for (days)</span>
            <Input
              inputMode="numeric"
              value={draft.memory_forget_after_days ?? ''}
              placeholder="Never (at least 7)"
              onChange={(e) =>
                setDraft((d) => ({
                  ...d,
                  memory_forget_after_days: parseBudget(e.target.value),
                }))
              }
            />
            <span className="block text-xs text-muted-foreground">
              Neither recalled by a node nor updated in that time. Checked hourly.
            </span>
          </label>
        </div>
        <div className="space-y-2 text-sm">
          <p className="font-medium">Allowed providers</p>
          <p className="text-muted-foreground">Leave all off to allow every provider.</p>
          <div className="flex flex-wrap gap-x-5 gap-y-2">
            {PROVIDERS.map(({ value, label }) => (
              <label key={value} className="flex items-center gap-2">
                <Switch
                  checked={draft.allowed_providers.includes(value)}
                  onCheckedChange={(on) => toggleProvider(value, on)}
                />
                {label}
              </label>
            ))}
          </div>
        </div>
        <div className="space-y-2 text-sm">
          <label className="flex items-center gap-2">
            <Switch
              checked={draft.redact_secrets}
              onCheckedChange={(redact_secrets) => setDraft((d) => ({ ...d, redact_secrets }))}
            />
            Remove API keys, tokens and private keys from the context sent to models
          </label>
          <label className="flex items-center gap-2">
            <Switch
              checked={draft.allow_code_exec}
              onCheckedChange={(allow_code_exec) => setDraft((d) => ({ ...d, allow_code_exec }))}
            />
            Let agents execute code where a node type asks for it
          </label>
        </div>
      </fieldset>
      {canEdit && (
        <div className="flex justify-end">
          <Button disabled={!dirty || save.isPending} onClick={() => save.mutate()}>
            Save guardrails
          </Button>
        </div>
      )}
    </GlassCard>
  );
}

/** Budgets, allowed providers and safety switches of the open workspace. */
export function GuardrailsCard() {
  const { current } = useCurrentWorkspace();
  return current ? <GuardrailsForm key={current.id} workspace={current} /> : null;
}
