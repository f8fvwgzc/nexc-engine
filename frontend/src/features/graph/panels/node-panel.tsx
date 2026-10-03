import { XIcon } from 'lucide-react';

import { NodeKindIcon } from '@/components/custom-ui/node-kind-icon';
import { Button } from '@/components/ui/button';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useIsMobile } from '@/hooks/use-mobile';
import type { GraphNode } from '@/schemas/graph';
import { useGraphStore } from '@/stores/graph-store';

import { NodeEditorForm } from './node-editor-form';
import { NodeRunOutput } from './node-run-output';

interface NodePanelProps {
  node: GraphNode | undefined;
  onDelete: (nodeId: string) => void;
}

function PanelBody({ node, onDelete }: { node: GraphNode; onDelete: () => void }) {
  const hasRunState = useGraphStore((s) => node.id in s.nodeStates);
  return (
    <Tabs defaultValue={hasRunState || node.output ? 'output' : 'edit'} className="gap-4">
      <TabsList className="w-full">
        <TabsTrigger value="edit">Edit</TabsTrigger>
        <TabsTrigger value="output">Run output</TabsTrigger>
      </TabsList>
      <TabsContent value="edit">
        <NodeEditorForm key={node.id} node={node} onDelete={onDelete} />
      </TabsContent>
      <TabsContent value="output">
        <NodeRunOutput node={node} />
      </TabsContent>
    </Tabs>
  );
}

function PanelTitle({ node }: { node: GraphNode }) {
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-brand/10 text-brand">
        <NodeKindIcon kind={node.kind} size={15} aria-hidden />
      </span>
      <span className="truncate">{node.title || 'Untitled'}</span>
    </span>
  );
}

/** Right-hand node inspector (bottom sheet on phones). */
export function NodePanel({ node, onDelete }: NodePanelProps) {
  const isMobile = useIsMobile();
  const selectNode = useGraphStore((s) => s.selectNode);
  const close = () => selectNode(null);

  if (isMobile) {
    return (
      <Sheet open={Boolean(node)} onOpenChange={(open) => !open && close()}>
        <SheetContent side="bottom" className="max-h-[85svh] overflow-y-auto">
          {node && (
            <>
              <SheetHeader>
                <SheetTitle>
                  <PanelTitle node={node} />
                </SheetTitle>
                <SheetDescription className="sr-only">
                  Edit the node and view its run output.
                </SheetDescription>
              </SheetHeader>
              <div className="px-4 pb-6">
                <PanelBody node={node} onDelete={() => onDelete(node.id)} />
              </div>
            </>
          )}
        </SheetContent>
      </Sheet>
    );
  }

  if (!node) return null;
  return (
    <aside
      aria-label="Node inspector"
      className="flex w-[380px] shrink-0 flex-col border-l bg-background/95 backdrop-blur motion-safe:animate-[fade-in_240ms_ease-out] xl:w-[420px]"
    >
      <header className="flex h-14 shrink-0 items-center justify-between gap-2 border-b px-4">
        <h2 className="min-w-0 text-sm font-semibold">
          <PanelTitle node={node} />
        </h2>
        <Button variant="ghost" size="icon-sm" aria-label="Close inspector" onClick={close}>
          <XIcon />
        </Button>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        <PanelBody key={node.id} node={node} onDelete={() => onDelete(node.id)} />
      </div>
    </aside>
  );
}
