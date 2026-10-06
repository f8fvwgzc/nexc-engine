import { labelFromKey } from '@/components/custom-ui/node-kind-meta';
import { EXECUTORS, type Executor, type Ontology } from '@/schemas/graph';

/** The kinds a node of this graph may have; `current` stays selectable even if its type was removed. */
export function kindOptions(ontology: Ontology, current: string) {
  const options = ontology.node_types.map((t) => ({ value: t.key, label: t.label || t.key }));
  return options.some((o) => o.value === current)
    ? options
    : [...options, { value: current, label: labelFromKey(current) }];
}

const EXECUTOR_LABELS: Record<Executor, string> = {
  llm: 'LLM (single call)',
  agent: 'Agent (tools, multi-turn)',
  symphony: 'Symphony (coding task)',
};

export const EXECUTOR_OPTIONS = EXECUTORS.map((value) => ({
  value,
  label: EXECUTOR_LABELS[value],
}));
