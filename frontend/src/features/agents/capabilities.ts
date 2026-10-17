import type { Capabilities, DataLabel } from '@/schemas/agent';

/** The tools the agent runtime offers, for the allow-list. */
export const RUNTIME_TOOLS = [
  'write_file',
  'read_file',
  'list_files',
  'make_docx',
  'run_python',
  'spawn_subagent',
  'finish',
] as const;

export const LABEL_META: Record<DataLabel, { label: string; hint: string }> = {
  public: { label: 'Public', hint: 'Anyone may read it' },
  internal: { label: 'Internal', hint: 'The default for everything the workspace holds' },
  confidential: { label: 'Confidential', hint: 'Only agents cleared for it' },
  restricted: { label: 'Restricted', hint: 'Named agents only' },
};

/** A short line per capability, for cards and tables. */
export function summarizeCapabilities(caps: Capabilities): string[] {
  const list = (items: string[] | null, any: string, none: string) =>
    items === null ? any : items.length === 0 ? none : items.join(', ');
  return [
    `Tools: ${list(caps.tools, 'every tool', 'none')}`,
    `Clearance: ${caps.data.labels.map((l) => LABEL_META[l].label.toLowerCase()).join(', ') || 'none'}`,
    `Memory: ${list(caps.data.memory_namespaces, 'all namespaces', 'none')}`,
    `Hosts: ${list(caps.egress_hosts, 'any', 'none')}`,
    `Counterparts: ${list(caps.counterparts, 'any role', 'none')}`,
    `Delegation: ${caps.delegation.max_depth} level(s) deep, ${caps.delegation.max_fan_out} per agent`,
    `Models: ${list(caps.models, 'any', 'none')}`,
  ];
}
