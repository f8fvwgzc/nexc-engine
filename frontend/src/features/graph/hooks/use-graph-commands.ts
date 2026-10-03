import { LayoutGridIcon, MaximizeIcon, PlayIcon, PlusIcon, SparklesIcon } from 'lucide-react';
import { useEffect } from 'react';

import { isRunActive } from '@/schemas/run';
import { useCommandStore, type CommandAction } from '@/stores/command-store';
import { useGraphStore } from '@/stores/graph-store';

interface GraphCommandHandlers {
  addNode: () => void;
  autoLayout: () => void;
  fit: () => void;
  requestPlan: () => void;
  run: () => void;
  nodeCount: number;
}

/** Contributes canvas actions to the ⌘K palette while the graph page is mounted. */
export function useGraphCommands(handlers: GraphCommandHandlers) {
  const planBusy = useGraphStore(
    (s) => s.plan?.status === 'streaming' || s.plan?.status === 'requesting',
  );
  const runBusy = useGraphStore((s) => (s.run ? isRunActive(s.run.status) : false));
  const { addNode, autoLayout, fit, requestPlan, run, nodeCount } = handlers;

  useEffect(() => {
    const actions: CommandAction[] = [
      { id: 'add-node', label: 'Create a node', icon: PlusIcon, shortcut: 'n', run: addNode },
      {
        id: 'plan',
        label: 'Request plan (LLM refinement)',
        icon: SparklesIcon,
        disabled: planBusy,
        run: requestPlan,
      },
      {
        id: 'run',
        label: 'Run graph',
        icon: PlayIcon,
        disabled: runBusy || nodeCount === 0,
        run,
      },
      {
        id: 'layout',
        label: 'Auto-layout',
        icon: LayoutGridIcon,
        shortcut: 'l',
        disabled: nodeCount === 0,
        run: autoLayout,
      },
      { id: 'fit', label: 'Fit graph to view', icon: MaximizeIcon, shortcut: 'f', run: fit },
    ];
    useCommandStore.getState().setPageActions(actions);
    return () => useCommandStore.getState().setPageActions([]);
  }, [addNode, autoLayout, fit, requestPlan, run, nodeCount, planBusy, runBusy]);
}
