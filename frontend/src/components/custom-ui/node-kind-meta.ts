import {
  BookOpenIcon,
  CodeXmlIcon,
  FileTextIcon,
  FlagIcon,
  HashIcon,
  ListTodoIcon,
  type LucideIcon,
} from 'lucide-react';

import type { NodeKind } from '@/schemas/graph';

export const NODE_KIND_META: Record<NodeKind, { label: string; icon: LucideIcon }> = {
  topic: { label: 'Topic', icon: HashIcon },
  task: { label: 'Task', icon: ListTodoIcon },
  research: { label: 'Research', icon: BookOpenIcon },
  code: { label: 'Code', icon: CodeXmlIcon },
  document: { label: 'Document', icon: FileTextIcon },
  output: { label: 'Output', icon: FlagIcon },
};
