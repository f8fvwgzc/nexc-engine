import { useId } from 'react';

import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { cn } from '@/lib/utils';
import { DATA_LABELS, MEMORY_NAMESPACES, type Capabilities, type DataLabel } from '@/schemas/agent';

import { LABEL_META, RUNTIME_TOOLS } from '../capabilities';

const NAMESPACE_META: Record<(typeof MEMORY_NAMESPACES)[number], string> = {
  user: "Members' own notes",
  graph: 'What graphs learned',
  node: 'Per-node memories',
};

/** A comma-separated list as a trimmed array; empty text is an empty list. */
function parseList(text: string): string[] {
  return text
    .split(/[,\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/**
 * One allow-list: a switch between "any" (`null`) and a list the user writes as comma-separated
 * entries. Capability lists stay short, so free text beats a picker for hosts, models and roles.
 */
function AllowList({
  label,
  hint,
  placeholder,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  placeholder: string;
  value: string[] | null;
  onChange: (next: string[] | null) => void;
}) {
  const id = useId();
  const restricted = value !== null;
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-3">
        <Label htmlFor={id} className="text-xs">
          {label}
        </Label>
        <label className="flex items-center gap-2 text-xs text-muted-foreground">
          <Switch
            size="sm"
            checked={restricted}
            onCheckedChange={(on) => onChange(on ? [] : null)}
            aria-label={`Restrict ${label.toLowerCase()}`}
          />
          {restricted ? 'Only these' : 'Any'}
        </label>
      </div>
      {restricted ? (
        <Input
          id={id}
          defaultValue={value.join(', ')}
          placeholder={placeholder}
          className="h-8 font-mono text-[13px]"
          onBlur={(e) => onChange(parseList(e.target.value))}
        />
      ) : (
        <p id={id} className="text-xs text-muted-foreground">
          {hint}
        </p>
      )}
    </div>
  );
}

/**
 * Edits an agent's capabilities document. Everything the policy engine decides from is here:
 * tools, clearance and memory namespaces, egress hosts, counterpart roles, delegation limits
 * and the model allow-list. The topic scope is edited by id on the API; it is shown, not edited.
 */
export function CapabilitiesEditor({
  value,
  onChange,
  className,
}: {
  value: Capabilities;
  onChange: (next: Capabilities) => void;
  className?: string;
}) {
  const toolsId = useId();
  const restrictedTools = value.tools !== null;
  const toggleTool = (tool: string, on: boolean) => {
    const current = value.tools ?? [];
    onChange({
      ...value,
      tools: on ? [...current.filter((t) => t !== tool), tool] : current.filter((t) => t !== tool),
    });
  };
  const toggleLabel = (label: DataLabel, on: boolean) =>
    onChange({
      ...value,
      data: {
        ...value.data,
        labels: DATA_LABELS.filter((l) => (l === label ? on : value.data.labels.includes(l))),
      },
    });
  const toggleNamespace = (namespace: (typeof MEMORY_NAMESPACES)[number], on: boolean) => {
    const current = value.data.memory_namespaces ?? [...MEMORY_NAMESPACES];
    const next = MEMORY_NAMESPACES.filter((n) => (n === namespace ? on : current.includes(n)));
    onChange({ ...value, data: { ...value.data, memory_namespaces: next } });
  };

  return (
    <div className={cn('space-y-5', className)}>
      <section className="space-y-2">
        <div className="flex items-center justify-between gap-3">
          <Label htmlFor={toolsId} className="text-xs">
            Tools
          </Label>
          <label className="flex items-center gap-2 text-xs text-muted-foreground">
            <Switch
              size="sm"
              checked={restrictedTools}
              onCheckedChange={(on) =>
                onChange({ ...value, tools: on ? [...RUNTIME_TOOLS] : null })
              }
              aria-label="Restrict tools"
            />
            {restrictedTools ? 'Only these' : 'Every tool'}
          </label>
        </div>
        <ul id={toolsId} className="grid grid-cols-2 gap-x-4 gap-y-1.5 sm:grid-cols-3">
          {RUNTIME_TOOLS.map((tool) => (
            <li key={tool} className="flex items-center gap-2">
              <Checkbox
                id={`${toolsId}-${tool}`}
                disabled={!restrictedTools}
                checked={!restrictedTools || (value.tools?.includes(tool) ?? false)}
                onCheckedChange={(on) => toggleTool(tool, on === true)}
              />
              <Label htmlFor={`${toolsId}-${tool}`} className="font-mono text-xs font-normal">
                {tool}
              </Label>
            </li>
          ))}
        </ul>
      </section>

      <section className="space-y-2">
        <p className="text-xs font-medium">Clearance</p>
        <ul className="grid grid-cols-2 gap-x-4 gap-y-1.5">
          {DATA_LABELS.map((label) => (
            <li key={label} className="flex items-start gap-2">
              <Checkbox
                id={`${toolsId}-label-${label}`}
                checked={value.data.labels.includes(label)}
                onCheckedChange={(on) => toggleLabel(label, on === true)}
                className="mt-0.5"
              />
              <Label
                htmlFor={`${toolsId}-label-${label}`}
                className="flex flex-col items-start gap-0 text-xs font-normal"
              >
                <span>{LABEL_META[label].label}</span>
                <span className="text-muted-foreground">{LABEL_META[label].hint}</span>
              </Label>
            </li>
          ))}
        </ul>
        {value.data.topics !== null && (
          <p className="text-xs text-muted-foreground">
            Limited to {value.data.topics.length} knowledge topic(s), set through the API.
          </p>
        )}
      </section>

      <section className="space-y-2">
        <div className="flex items-center justify-between gap-3">
          <p className="text-xs font-medium">Memory namespaces</p>
          <label className="flex items-center gap-2 text-xs text-muted-foreground">
            <Switch
              size="sm"
              checked={value.data.memory_namespaces !== null}
              onCheckedChange={(on) =>
                onChange({
                  ...value,
                  data: { ...value.data, memory_namespaces: on ? [...MEMORY_NAMESPACES] : null },
                })
              }
              aria-label="Restrict memory namespaces"
            />
            {value.data.memory_namespaces !== null ? 'Only these' : 'All'}
          </label>
        </div>
        <ul className="grid grid-cols-3 gap-x-4 gap-y-1.5">
          {MEMORY_NAMESPACES.map((namespace) => (
            <li key={namespace} className="flex items-center gap-2">
              <Checkbox
                id={`${toolsId}-ns-${namespace}`}
                disabled={value.data.memory_namespaces === null}
                checked={value.data.memory_namespaces?.includes(namespace) ?? true}
                onCheckedChange={(on) => toggleNamespace(namespace, on === true)}
              />
              <Label htmlFor={`${toolsId}-ns-${namespace}`} className="text-xs font-normal">
                {NAMESPACE_META[namespace]}
              </Label>
            </li>
          ))}
        </ul>
      </section>

      <AllowList
        label="Egress hosts"
        hint="May call out to any host (nothing calls out yet; the gateway comes later)."
        placeholder="api.example.com, *.internal.example"
        value={value.egress_hosts}
        onChange={(egress_hosts) => onChange({ ...value, egress_hosts })}
      />
      <AllowList
        label="Counterpart roles"
        hint="May delegate to a sub-agent of any role."
        placeholder="researcher, editor"
        value={value.counterparts}
        onChange={(counterparts) => onChange({ ...value, counterparts })}
      />
      <AllowList
        label="Models"
        hint="May run on whatever model the workspace resolves."
        placeholder="claude-opus-5, claude-sonnet-5-5"
        value={value.models}
        onChange={(models) => onChange({ ...value, models })}
      />

      <section className="grid grid-cols-2 gap-4">
        <div className="space-y-1.5">
          <Label htmlFor={`${toolsId}-depth`} className="text-xs">
            Delegation depth
          </Label>
          <Input
            id={`${toolsId}-depth`}
            type="number"
            inputMode="numeric"
            min={0}
            max={8}
            className="h-8"
            value={value.delegation.max_depth}
            onChange={(e) =>
              onChange({
                ...value,
                delegation: { ...value.delegation, max_depth: e.target.valueAsNumber },
              })
            }
          />
          <p className="text-xs text-muted-foreground">Levels of sub-agents below it; 0 = none.</p>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor={`${toolsId}-fanout`} className="text-xs">
            Sub-agents per agent
          </Label>
          <Input
            id={`${toolsId}-fanout`}
            type="number"
            inputMode="numeric"
            min={0}
            max={32}
            className="h-8"
            value={value.delegation.max_fan_out}
            onChange={(e) =>
              onChange({
                ...value,
                delegation: { ...value.delegation, max_fan_out: e.target.valueAsNumber },
              })
            }
          />
        </div>
      </section>
    </div>
  );
}
