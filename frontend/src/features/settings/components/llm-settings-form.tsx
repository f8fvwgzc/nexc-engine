import { zodResolver } from '@hookform/resolvers/zod';
import { useSuspenseQuery } from '@tanstack/react-query';
import { KeyRoundIcon, SaveIcon } from 'lucide-react';
import { useForm, useWatch } from 'react-hook-form';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { FormField } from '@/components/custom-ui/form-field';
import { GlassCard } from '@/components/custom-ui/glass-card';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { Button } from '@/components/ui/button';
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import {
  llmSettingsInputSchema,
  type LlmProvider,
  type LlmSettings,
  type LlmSettingsInput,
} from '@/schemas/settings';

import { llmSettingsQuery, type UpdateLlmSettingsBody } from '../api';
import { useUpdateLlmSettings } from '../hooks/use-update-llm-settings';

const PROVIDERS: { value: LlmProvider; label: string }[] = [
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'openai_compatible', label: 'OpenAI-compatible' },
  { value: 'claude_code', label: 'Claude Code CLI (your Claude login)' },
  { value: 'demo', label: 'Demo (offline, simulated outputs)' },
];

const SOURCE_LABEL: Record<LlmSettings['source'], string> = {
  user: 'your stored key',
  server: 'the server default key',
  none: 'no key',
};

function toForm(s: LlmSettings): LlmSettingsInput {
  return { provider: s.provider, model: s.model, base_url: s.base_url ?? '', api_key: '' };
}

const FIELDS = ['provider', 'model', 'base_url', 'api_key'] as const;

export function LlmSettingsForm() {
  const { data: settings } = useSuspenseQuery(llmSettingsQuery());
  const update = useUpdateLlmSettings();
  const form = useForm<LlmSettingsInput>({
    resolver: zodResolver(llmSettingsInputSchema),
    defaultValues: toForm(settings),
  });
  const provider = useWatch({ control: form.control, name: 'provider' });
  const isDemo = provider === 'demo';
  const isClaudeCode = provider === 'claude_code';
  // Neither the demo nor the Claude Code CLI uses a base URL or an API key.
  const keyless = isDemo || isClaudeCode;

  const save = (body: UpdateLlmSettingsBody, message: string) =>
    update.mutate(body, {
      onSuccess: (saved) => {
        form.reset(toForm(saved));
        toast.success(message);
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
    save(body, 'LLM settings saved');
  });

  return (
    <GlassCard className="p-5 sm:p-6">
      <form noValidate onSubmit={(e) => void submit(e)} className="space-y-6">
        <div className="space-y-1">
          <h2 className="font-medium">LLM provider</h2>
          <p className="text-sm text-muted-foreground">
            Currently using{' '}
            <span className="font-medium text-foreground">{SOURCE_LABEL[settings.source]}</span>
            {settings.key_hint && (
              <>
                {' '}
                (<code className="font-mono">{settings.key_hint}</code>)
              </>
            )}
            .
          </p>
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
            <FormField control={form.control} name="model" label="Model">
              {(field) => (
                <Input
                  {...field}
                  placeholder={isDemo ? 'demo' : isClaudeCode ? 'sonnet' : 'claude-opus-5'}
                />
              )}
            </FormField>
          </div>
          {isDemo ? (
            <p className="rounded-lg border border-brand/30 bg-brand/5 p-3 text-sm text-muted-foreground">
              The demo provider runs fully offline with deterministic, clearly-labelled outputs —
              great for trying planning and runs without an API key.
            </p>
          ) : isClaudeCode ? (
            <p className="rounded-lg border border-brand/30 bg-brand/5 p-3 text-sm text-muted-foreground">
              Runs the <code className="font-mono">claude</code> CLI installed on the server with
              its own login — no API key needed. Use a model alias such as{' '}
              <code className="font-mono">sonnet</code> or <code className="font-mono">haiku</code>{' '}
              (cheaper) or a full model name. Works when the backend and runtime run on a machine
              where Claude Code is installed and logged in (e.g. <code>make dev</code>).
            </p>
          ) : (
            <>
              <FormField
                control={form.control}
                name="base_url"
                label="Base URL"
                description="Leave blank for the provider default. Required for most OpenAI-compatible servers."
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
                  settings.has_api_key
                    ? `A key ending ${settings.key_hint ?? ''} is stored. Leave blank to keep it.`
                    : 'Stored encrypted on the server and never shown again.'
                }
              >
                {(field) => (
                  <Input
                    {...field}
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    placeholder={settings.has_api_key ? '••••••••••••' : 'sk-…'}
                  />
                )}
              </FormField>
            </>
          )}
        </FieldGroup>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {settings.has_api_key && settings.source === 'user' && !keyless && (
            <Button
              type="button"
              variant="ghost"
              className="mr-auto text-destructive"
              disabled={update.isPending}
              onClick={() =>
                save(
                  {
                    provider: settings.provider,
                    model: settings.model,
                    base_url: settings.base_url,
                    api_key: '',
                  },
                  'Stored API key removed',
                )
              }
            >
              <KeyRoundIcon />
              Remove stored key
            </Button>
          )}
          <AnimatedButton type="submit" loading={update.isPending} loadingText="Saving…">
            <SaveIcon />
            Save
          </AnimatedButton>
        </div>
      </form>
    </GlassCard>
  );
}
