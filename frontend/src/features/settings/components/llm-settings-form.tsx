import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from '@tanstack/react-query';
import { KeyRoundIcon, SaveIcon, UnplugIcon } from 'lucide-react';
import { useId, type ReactNode } from 'react';
import { useForm, useWatch } from 'react-hook-form';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { FormField } from '@/components/custom-ui/form-field';
import { GlassCard } from '@/components/custom-ui/glass-card';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { Button } from '@/components/ui/button';
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import { qk } from '@/lib/query-keys';
import {
  llmSettingsInputSchema,
  type LlmProvider,
  type LlmSettings,
  type LlmSettingsInput,
  type ModelCatalog,
} from '@/schemas/settings';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

import {
  disconnectLlm,
  llmModelsQuery,
  llmSettingsQuery,
  removeWorkspaceLlm,
  updateLlmSettings,
  updateWorkspaceLlm,
  workspaceLlmQuery,
  type UpdateLlmSettingsBody,
} from '../api';

const PROVIDERS: { value: LlmProvider; label: string }[] = [
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'openai_compatible', label: 'OpenAI-compatible' },
  { value: 'claude_code', label: 'Claude Code CLI (the server’s Claude login)' },
  { value: 'demo', label: 'Demo (offline, simulated outputs)' },
];

const SCOPE_LABEL: Record<LlmSettings['scope'], string> = {
  user: 'your own account',
  workspace: 'the workspace’s credential',
  server: 'the server default',
};

interface FormDefaults {
  provider: LlmProvider;
  model: string;
  base_url: string | null;
  has_api_key: boolean;
  key_hint: string | null;
}

const toForm = (s: FormDefaults): LlmSettingsInput => ({
  provider: s.provider,
  model: s.model,
  base_url: s.base_url ?? '',
  api_key: '',
});

const FIELDS = ['provider', 'model', 'base_url', 'api_key'] as const;

interface CredentialFormProps {
  heading: string;
  intro: ReactNode;
  defaults: FormDefaults;
  /** Models the provider offers, to suggest while typing. */
  catalog?: ModelCatalog;
  save: (body: UpdateLlmSettingsBody) => Promise<LlmSettings>;
  savedMessage: string;
  onSaved: () => void;
  /** Extra control on the left of the footer (disconnect / remove). */
  footer?: ReactNode;
}

/** Provider, model, endpoint and key of one credential — a user's or a workspace's. */
function CredentialForm({
  heading,
  intro,
  defaults,
  catalog,
  save,
  savedMessage,
  onSaved,
  footer,
}: CredentialFormProps) {
  const modelsId = useId();
  const form = useForm<LlmSettingsInput>({
    resolver: zodResolver(llmSettingsInputSchema),
    defaultValues: toForm(defaults),
  });
  const provider = useWatch({ control: form.control, name: 'provider' });
  const isDemo = provider === 'demo';
  const isClaudeCode = provider === 'claude_code';
  // Neither the demo nor the Claude Code CLI uses a base URL or an API key.
  const keyless = isDemo || isClaudeCode;
  const update = useMutation({
    mutationFn: save,
    meta: { errorToast: false },
    onSuccess: (saved) => {
      form.reset(toForm(saved));
      toast.success(savedMessage);
      onSaved();
    },
    onError: (error) => {
      if (!applyProblemToForm(error, form.setError, FIELDS)) toast.error(errorMessage(error));
    },
  });
  const submit = form.handleSubmit((values) => {
    const body: UpdateLlmSettingsBody = {
      provider: values.provider,
      model: values.model,
      base_url: keyless ? null : values.base_url || null,
    };
    if (!keyless && values.api_key) body.api_key = values.api_key;
    update.mutate(body);
  });
  // Suggestions only make sense for the provider they were fetched from.
  const models = catalog?.provider === provider ? catalog.models : [];

  return (
    <GlassCard className="p-5 sm:p-6">
      <form noValidate onSubmit={(e) => void submit(e)} className="space-y-6">
        <div className="space-y-1">
          <h2 className="font-medium">{heading}</h2>
          <p className="text-sm text-muted-foreground">{intro}</p>
        </div>
        <FieldGroup className="gap-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <FormField control={form.control} name="provider" label="Provider">
              {({ value, onChange, ...field }) => (
                <OptionSelect
                  {...field}
                  value={value}
                  onValueChange={onChange}
                  options={PROVIDERS}
                />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="model"
              label="Model"
              description={
                models.length > 0
                  ? `${models.length} models offered by this provider right now; newest first.`
                  : catalog?.provider === provider
                    ? (catalog.note ?? undefined)
                    : undefined
              }
            >
              {(field) => (
                <Input
                  {...field}
                  list={modelsId}
                  autoComplete="off"
                  placeholder={isDemo ? 'demo' : isClaudeCode ? 'sonnet' : 'model id'}
                />
              )}
            </FormField>
            <datalist id={modelsId}>
              {models.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.recent ? `${model.name} · new` : model.name}
                </option>
              ))}
            </datalist>
          </div>
          {isDemo ? (
            <p className="rounded-lg border bg-muted/40 p-3 text-sm text-muted-foreground">
              The demo provider runs fully offline with deterministic, clearly-labelled outputs —
              useful for trying planning and runs without an API key.
            </p>
          ) : isClaudeCode ? (
            <p className="rounded-lg border bg-muted/40 p-3 text-sm text-muted-foreground">
              Runs the <code className="font-mono">claude</code> CLI installed on the machine that
              hosts Nexc, with that machine’s login — no API key, and its usage is spent on that
              login. Use an alias such as <code className="font-mono">sonnet</code> or{' '}
              <code className="font-mono">haiku</code>, or a full model name. Meant for running Nexc
              on your own computer.
            </p>
          ) : (
            <>
              <FormField
                control={form.control}
                name="base_url"
                label="Base URL"
                description="Leave blank for the provider default. For a self-hosted or open-weight model, enter its OpenAI-compatible URL, e.g. http://localhost:11434/v1."
              >
                {(field) => (
                  <Input {...field} type="url" placeholder="https://api.example.com/v1" />
                )}
              </FormField>
              <FormField
                control={form.control}
                name="api_key"
                label="API key"
                description={
                  defaults.has_api_key
                    ? `A key ending ${defaults.key_hint ?? ''} is stored. Leave blank to keep it.`
                    : 'Stored encrypted on the server and never shown again.'
                }
              >
                {(field) => (
                  <Input
                    {...field}
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    placeholder={defaults.has_api_key ? '••••••••••••' : 'sk-…'}
                  />
                )}
              </FormField>
            </>
          )}
        </FieldGroup>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {footer && <div className="mr-auto">{footer}</div>}
          <AnimatedButton type="submit" loading={update.isPending} loadingText="Saving…">
            <SaveIcon />
            Save
          </AnimatedButton>
        </div>
      </form>
    </GlassCard>
  );
}

function useRefreshLlm() {
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({ queryKey: qk.settings.llm });
    // demo_mode may have flipped.
    void queryClient.invalidateQueries({ queryKey: qk.orchestrator });
  };
}

/** The caller's own account. It takes precedence over the workspace's credential. */
function OwnAccountCard({ workspace }: { workspace: Workspace | undefined }) {
  const { data: settings } = useSuspenseQuery(llmSettingsQuery(workspace?.id));
  const { data: catalog } = useQuery(llmModelsQuery(workspace?.id));
  const refresh = useRefreshLlm();
  const disconnect = useMutation({
    mutationFn: disconnectLlm,
    meta: { successMessage: 'Your account was disconnected' },
    onSuccess: refresh,
  });
  const own = settings.scope === 'user';
  return (
    <CredentialForm
      // Remount when the effective configuration changes owner, so the form shows it.
      key={`${settings.scope}:${settings.provider}:${settings.model}:${settings.key_hint ?? ''}`}
      heading="Your AI account"
      intro={
        <>
          Your work currently runs on{' '}
          <span className="font-medium text-foreground">{SCOPE_LABEL[settings.scope]}</span>
          {settings.key_hint && (
            <>
              {' '}
              (key <code className="font-mono">{settings.key_hint}</code>)
            </>
          )}
          . Saving here connects your own account: it is used for everything you run, and its usage
          is yours.
        </>
      }
      defaults={own ? settings : { ...settings, has_api_key: false, key_hint: null }}
      catalog={catalog}
      save={updateLlmSettings}
      savedMessage="Your account is connected"
      onSaved={refresh}
      footer={
        own && (
          <Button
            type="button"
            variant="ghost"
            className="text-destructive"
            disabled={disconnect.isPending}
            onClick={() => disconnect.mutate()}
          >
            <UnplugIcon />
            Disconnect my account
          </Button>
        )
      }
    />
  );
}

/** The credential members fall back to; only workspace admins can change it. */
function WorkspaceCredentialCard({ workspace }: { workspace: Workspace }) {
  const { data: stored } = useSuspenseQuery(workspaceLlmQuery(workspace.id));
  const refresh = useRefreshLlm();
  const remove = useMutation({
    mutationFn: () => removeWorkspaceLlm(workspace.id),
    meta: { successMessage: 'Workspace credential removed' },
    onSuccess: refresh,
  });
  if (!isWorkspaceAdmin(workspace.role)) {
    return (
      <GlassCard className="space-y-1 p-5 sm:p-6">
        <h2 className="font-medium">Workspace credential</h2>
        <p className="text-sm text-muted-foreground">
          {stored
            ? `${workspace.name} provides ${stored.provider} · ${stored.model} for members who have not connected their own account.`
            : `${workspace.name} has no shared credential. Connect your own account above, or ask a workspace admin to add one.`}
        </p>
      </GlassCard>
    );
  }
  return (
    <CredentialForm
      key={`${workspace.id}:${stored?.provider ?? ''}:${stored?.model ?? ''}:${stored?.key_hint ?? ''}`}
      heading="Workspace credential"
      intro={
        stored
          ? `Used for members of ${workspace.name} who have not connected their own account. Its usage is the workspace’s.`
          : `${workspace.name} has no shared credential yet. Add one so members can work without bringing their own key.`
      }
      defaults={
        stored ?? {
          provider: 'anthropic',
          model: '',
          base_url: null,
          has_api_key: false,
          key_hint: null,
        }
      }
      save={(body) => updateWorkspaceLlm(workspace.id, body)}
      savedMessage="Workspace credential saved"
      onSaved={refresh}
      footer={
        stored && (
          <Button
            type="button"
            variant="ghost"
            className="text-destructive"
            disabled={remove.isPending}
            onClick={() => remove.mutate()}
          >
            <KeyRoundIcon />
            Remove workspace credential
          </Button>
        )
      }
    />
  );
}

/** AI credentials: the caller's own account, and the workspace's shared one. */
export function LlmSettingsForm() {
  const { current } = useCurrentWorkspace();
  return (
    <div className="space-y-6">
      <OwnAccountCard workspace={current} />
      {current && <WorkspaceCredentialCard workspace={current} />}
    </div>
  );
}
