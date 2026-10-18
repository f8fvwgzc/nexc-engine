import { FileTextIcon } from 'lucide-react';

import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { DATA_LABELS, type DataLabel } from '@/schemas/agent';
import { isRecordingOn, type Recording } from '@/schemas/workspace';

const KINDS: { key: 'prompts' | 'completions' | 'tool_io'; label: string; hint: string }[] = [
  {
    key: 'prompts',
    label: 'Prompts',
    hint: 'What every model turn was given: the system prompt and the task, then what was added since the last turn.',
  },
  {
    key: 'completions',
    label: 'Completions',
    hint: 'What the model answered: its text and the tool calls it asked for, with their arguments.',
  },
  {
    key: 'tool_io',
    label: 'Tool input and output',
    hint: "The arguments and the result of every tool call, the gateway's included.",
  },
];

interface RecordingFieldsProps {
  value: Recording;
  onChange: (next: Recording) => void;
}

/**
 * The recording part of the guardrails: which kinds of payload are kept, for how long, and which
 * labels are redacted. Off by default; spans carry digests only until a kind is switched on.
 */
export function RecordingFields({ value, onChange }: RecordingFieldsProps) {
  const set = (patch: Partial<Recording>) => onChange({ ...value, ...patch });
  const toggleLabel = (label: DataLabel, on: boolean) =>
    set({
      redact_labels: on
        ? [...value.redact_labels.filter((l) => l !== label), label]
        : value.redact_labels.filter((l) => l !== label),
    });
  return (
    <fieldset className="space-y-3 rounded-lg border p-4 text-sm">
      <legend className="flex items-center gap-2 px-1 font-medium">
        <FileTextIcon className="size-4" aria-hidden />
        Recording
      </legend>
      <p className="text-muted-foreground">
        Off, spans carry digests only. On, what agents say and hear is kept sealed beside each span
        for the Sessions page, after credentials are scrubbed and the passages of redacted labels
        are replaced by their citation; the run&apos;s owner and admins may read it, and every read
        is audited.
      </p>
      <div className="space-y-2">
        {KINDS.map(({ key, label, hint }) => (
          <label key={key} className="flex items-start gap-2">
            <Switch
              checked={value[key]}
              onCheckedChange={(on) => set({ [key]: on })}
              aria-label={`Record ${label.toLowerCase()}`}
            />
            <span>
              <span className="font-medium">{label}</span>
              <span className="block text-xs text-muted-foreground">{hint}</span>
            </span>
          </label>
        ))}
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <label className="space-y-1.5">
          <span className="font-medium">Keep for (days)</span>
          <Input
            inputMode="numeric"
            value={value.retention_days}
            disabled={!isRecordingOn(value)}
            onChange={(e) => {
              const digits = e.target.value.replace(/\D/g, '');
              set({ retention_days: digits ? Number(digits) : 0 });
            }}
            aria-invalid={value.retention_days < 1 || value.retention_days > 3650}
          />
          <span className="block text-xs text-muted-foreground">
            1 to 3650; older payloads are deleted every minute.
          </span>
        </label>
        <div className="space-y-1.5">
          <span className="font-medium">Redact labels</span>
          <div className="flex flex-wrap gap-x-4 gap-y-2">
            {DATA_LABELS.map((label) => (
              <label key={label} className="flex items-center gap-2 capitalize">
                <Checkbox
                  checked={value.redact_labels.includes(label)}
                  disabled={!isRecordingOn(value)}
                  onCheckedChange={(on) => toggleLabel(label, on === true)}
                  aria-label={`Redact ${label} passages`}
                />
                {label}
              </label>
            ))}
          </div>
          <span className="block text-xs text-muted-foreground">
            Passages and memories of these labels are replaced by their citation id.
          </span>
        </div>
      </div>
    </fieldset>
  );
}
