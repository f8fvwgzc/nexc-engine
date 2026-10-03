import { useShallow } from 'zustand/react/shallow';

import { Seo } from '@/components/seo/seo';
import { isRunActive } from '@/schemas/run';
import { selectRunProgress, useGraphStore } from '@/stores/graph-store';

/** Document title that reflects live activity, e.g. "▶ Running 3/7 · Report — nexc-engine". */
export function GraphSeo({ name, description }: { name: string; description: string }) {
  const runActive = useGraphStore((s) => (s.run ? isRunActive(s.run.status) : false));
  const progress = useGraphStore(useShallow(selectRunProgress));
  const planning = useGraphStore(
    (s) => s.plan?.status === 'streaming' || s.plan?.status === 'requesting',
  );

  let title = `Graph · ${name}`;
  if (runActive && progress) title = `▶ Running ${progress.done}/${progress.total} · ${name}`;
  else if (planning) title = `✦ Planning · ${name}`;

  return (
    <Seo title={title} description={description || `Graph “${name}” in nexc-engine.`} noIndex />
  );
}
