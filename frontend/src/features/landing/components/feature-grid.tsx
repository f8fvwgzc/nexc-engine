import {
  GitBranchIcon,
  PlayCircleIcon,
  SparklesIcon,
  UsersIcon,
  type LucideIcon,
} from 'lucide-react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { Stagger } from '@/components/custom-ui/motion';

const FEATURES: { icon: LucideIcon; title: string; body: string }[] = [
  {
    icon: GitBranchIcon,
    title: 'Graph-native planning',
    body: 'Break a goal into topics and tasks on an Obsidian-style canvas. Link with [[wikilinks]] — dependencies are detected for you.',
  },
  {
    icon: SparklesIcon,
    title: 'Streaming LLM plans',
    body: 'Ask for a plan and watch proposed nodes stream onto the canvas. Nothing changes until you apply it.',
  },
  {
    icon: PlayCircleIcon,
    title: 'Run the whole DAG',
    body: 'Nodes execute in dependency order with live status, streamed output, token and cost tracking, retries and caching.',
  },
  {
    icon: UsersIcon,
    title: 'Agents with budgets',
    body: 'An org chart of agents with roles and token budgets. Files they produce are downloadable artifacts.',
  },
];

export function FeatureGrid() {
  return (
    <Stagger className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
      {FEATURES.map(({ icon: Icon, title, body }) => (
        <GlassCard key={title} className="h-full space-y-3 p-5">
          <span className="flex size-9 items-center justify-center rounded-lg bg-brand/10 text-brand">
            <Icon className="size-4" aria-hidden />
          </span>
          <h3 className="font-medium tracking-tight">{title}</h3>
          <p className="text-sm leading-relaxed text-muted-foreground">{body}</p>
        </GlassCard>
      ))}
    </Stagger>
  );
}
