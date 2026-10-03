import { useQuery } from '@tanstack/react-query';
import { ArrowRightIcon, LayoutTemplateIcon } from 'lucide-react';
import { useState } from 'react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { Stagger } from '@/components/custom-ui/motion';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { Spinner } from '@/components/ui/spinner';
import type { GraphTemplate } from '@/schemas/template';

import { templatesQuery } from '../api';
import { useCreateFromTemplate } from '../hooks/use-graph-mutations';
import { TemplateTopicDialog } from './template-topic-dialog';

function TemplateCard({
  template,
  busy,
  disabled,
  onUse,
}: {
  template: GraphTemplate;
  busy: boolean;
  disabled: boolean;
  onUse: () => void;
}) {
  return (
    <GlassCard interactive className="h-full">
      <button
        type="button"
        onClick={onUse}
        disabled={disabled}
        aria-busy={busy}
        className="group flex h-full w-full flex-col gap-3 rounded-[inherit] p-4 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-wait"
      >
        <div className="flex items-center justify-between gap-2">
          <Badge variant="secondary" className="capitalize">
            {template.category}
          </Badge>
          <span className="text-xs text-muted-foreground tabular-nums">
            {template.node_count} nodes
          </span>
        </div>
        <div className="space-y-1">
          <h3 className="font-medium tracking-tight">{template.name}</h3>
          <p className="line-clamp-2 text-sm text-muted-foreground">{template.description}</p>
        </div>
        <div className="mt-auto flex items-end justify-between gap-2">
          <div className="flex flex-wrap gap-1">
            {template.tags.slice(0, 3).map((tag) => (
              <span
                key={tag}
                className="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground"
              >
                {tag}
              </span>
            ))}
          </div>
          <span className="flex shrink-0 items-center gap-1 text-xs font-medium text-brand">
            {busy ? <Spinner className="size-3.5" /> : 'Use'}
            {!busy && (
              <ArrowRightIcon className="size-3.5 transition-transform group-hover:translate-x-0.5" />
            )}
          </span>
        </div>
      </button>
    </GlassCard>
  );
}

/** "Start from a template": pick one, say what it is about, and the canvas opens. */
export function TemplateGallery({ heading = true }: { heading?: boolean }) {
  const { data: templates, isPending, isError } = useQuery(templatesQuery());
  const createFromTemplate = useCreateFromTemplate();
  const [chosen, setChosen] = useState<GraphTemplate | null>(null);
  if (isError || templates?.length === 0) return null;

  return (
    <section aria-labelledby="templates-heading" className="space-y-3">
      {heading && (
        <h2 id="templates-heading" className="flex items-center gap-2 text-sm font-medium">
          <LayoutTemplateIcon className="size-4 text-brand" aria-hidden />
          Start from a template
        </h2>
      )}
      {isPending ? (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="h-40 rounded-xl" />
          ))}
        </div>
      ) : (
        <Stagger className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {templates.map((template) => (
            <TemplateCard
              key={template.id}
              template={template}
              disabled={createFromTemplate.isPending}
              busy={
                createFromTemplate.isPending &&
                createFromTemplate.variables.template_id === template.id
              }
              onUse={() => setChosen(template)}
            />
          ))}
        </Stagger>
      )}
      <TemplateTopicDialog
        template={chosen}
        pending={createFromTemplate.isPending}
        onOpenChange={(open) => !open && setChosen(null)}
        onCreate={(topic) =>
          chosen &&
          createFromTemplate.mutate(
            { template_id: chosen.id, topic },
            { onSuccess: () => setChosen(null) },
          )
        }
      />
    </section>
  );
}
