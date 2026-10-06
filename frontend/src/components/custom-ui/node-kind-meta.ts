import {
  BookOpenIcon,
  BrainIcon,
  CalendarIcon,
  ChartColumnIcon,
  ChartLineIcon,
  CircleDotIcon,
  CodeXmlIcon,
  DatabaseIcon,
  FileTextIcon,
  FlagIcon,
  FlaskConicalIcon,
  GlobeIcon,
  HashIcon,
  LightbulbIcon,
  ListTodoIcon,
  type LucideIcon,
  MessageSquareIcon,
  PuzzleIcon,
  ScaleIcon,
  SearchIcon,
  ShieldIcon,
  TargetIcon,
  TrendingUpIcon,
  UsersIcon,
  WrenchIcon,
  ZapIcon,
} from 'lucide-react';
import { createContext, useContext } from 'react';

import { EMPTY_ONTOLOGY, type NodeType, type Ontology, type RelationType } from '@/schemas/graph';

/** The icon names a node type may use; anything else renders as a dot. */
export const NODE_TYPE_ICONS: Record<string, LucideIcon> = {
  'bar-chart': ChartColumnIcon,
  'book-open': BookOpenIcon,
  brain: BrainIcon,
  calendar: CalendarIcon,
  chart: ChartLineIcon,
  circle: CircleDotIcon,
  code: CodeXmlIcon,
  database: DatabaseIcon,
  'file-text': FileTextIcon,
  flag: FlagIcon,
  flask: FlaskConicalIcon,
  globe: GlobeIcon,
  hash: HashIcon,
  lightbulb: LightbulbIcon,
  'list-todo': ListTodoIcon,
  message: MessageSquareIcon,
  puzzle: PuzzleIcon,
  scale: ScaleIcon,
  search: SearchIcon,
  shield: ShieldIcon,
  target: TargetIcon,
  'trending-up': TrendingUpIcon,
  users: UsersIcon,
  wrench: WrenchIcon,
  zap: ZapIcon,
};

export const NODE_TYPE_ICON_NAMES = Object.keys(NODE_TYPE_ICONS);

/** The ontology of the graph being shown; node and edge kinds resolve against it. */
export const OntologyContext = createContext<Ontology>(EMPTY_ONTOLOGY);

export function useOntology(): Ontology {
  return useContext(OntologyContext);
}

/** `market_signal` → `Market signal`: what a kind is called when its type is not (yet) known. */
export function labelFromKey(key: string): string {
  const text = key.replaceAll('_', ' ');
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export interface NodeTypeMeta {
  label: string;
  description: string;
  /** CSS colour, or undefined to inherit the surrounding colour. */
  color: string | undefined;
  icon: LucideIcon;
}

export function nodeTypeMeta(type: NodeType | undefined, kind: string): NodeTypeMeta {
  return {
    label: type?.label || labelFromKey(kind),
    description: type?.description ?? '',
    color: type?.color || undefined,
    icon: NODE_TYPE_ICONS[type?.icon ?? ''] ?? CircleDotIcon,
  };
}

/** How a node kind looks and reads in the current graph. */
export function useNodeType(kind: string): NodeTypeMeta {
  const type = useOntology().node_types.find((t) => t.key === kind);
  return nodeTypeMeta(type, kind);
}

export function relationLabel(relation: RelationType | undefined, kind: string): string {
  return relation?.label || labelFromKey(kind);
}
