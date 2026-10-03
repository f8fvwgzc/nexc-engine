import { NODE_KIND_META } from '@/components/custom-ui/node-kind-meta';
import { EXECUTORS, NODE_KINDS, type Executor, type NodeKind } from '@/schemas/graph';

export const KIND_OPTIONS = NODE_KINDS.map((kind) => ({
  value: kind,
  label: NODE_KIND_META[kind].label,
})) satisfies { value: NodeKind; label: string }[];

const EXECUTOR_LABELS: Record<Executor, string> = {
  llm: 'LLM (single call)',
  agent: 'Agent (tools, multi-turn)',
  symphony: 'Symphony (coding task)',
};

export const EXECUTOR_OPTIONS = EXECUTORS.map((value) => ({
  value,
  label: EXECUTOR_LABELS[value],
}));
