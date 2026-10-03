import { PlusIcon, SparklesIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { FadeIn } from '@/components/custom-ui/motion';
import { GradientText } from '@/components/custom-ui/gradient-text';
import { useAuthStore } from '@/stores/auth-store';

import { TemplateGallery } from './template-gallery';

/** First-time dashboard: welcome copy, templates, or a blank graph. */
export function FirstRunHero() {
  const name = useAuthStore((s) => s.user?.name.split(' ')[0]);
  return (
    <div className="space-y-10">
      <FadeIn className="relative overflow-hidden rounded-2xl border bg-gradient-to-br from-brand/10 via-transparent to-brand-2/10 p-6 sm:p-10">
        <div className="max-w-2xl space-y-4">
          <p className="inline-flex items-center gap-1.5 rounded-full border bg-background/60 px-3 py-1 text-xs text-muted-foreground backdrop-blur">
            <SparklesIcon className="size-3.5 text-brand" aria-hidden />
            Welcome{name ? `, ${name}` : ''}
          </p>
          <h1 className="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
            Turn a big goal into a <GradientText>graph of tasks</GradientText> — then run it.
          </h1>
          <p className="text-pretty text-muted-foreground">
            Sketch topics and tasks, let the planner refine them, watch dependencies appear in
            realtime, and execute the whole DAG with streaming output and downloadable artifacts.
          </p>
          <div className="flex flex-wrap gap-2 pt-2">
            <AnimatedButton glow size="lg" asChild>
              <Link to="/app?new=1">
                <PlusIcon />
                Blank graph
              </Link>
            </AnimatedButton>
          </div>
        </div>
      </FadeIn>
      <TemplateGallery />
    </div>
  );
}
