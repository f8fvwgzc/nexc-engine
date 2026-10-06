import { PlusIcon, ShapesIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { NODE_TYPE_ICON_NAMES } from '@/components/custom-ui/node-kind-meta';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { errorMessage } from '@/lib/api/errors';
import type { Graph, NodeType, Ontology, RelationType } from '@/schemas/graph';

import { useReplaceOntology } from '../hooks/use-graph-mutations';
import { ToolbarButton } from './toolbar-button';

/** A row being edited: `uid` is stable while `key` may still be empty for a new type. */
type Draft<T> = T & { uid: string; isNew: boolean };

let nextUid = 0;
const draft = <T,>(value: T, isNew: boolean): Draft<T> => ({
  ...value,
  uid: `t${nextUid++}`,
  isNew,
});

const NEW_NODE_TYPE: NodeType = {
  key: '',
  label: '',
  description: '',
  color: '#6366f1',
  icon: 'circle',
  default_role: '',
  default_executor: 'llm',
  stage: 2,
  produces_artifact: false,
  allow_code_exec: false,
};
const NEW_RELATION_TYPE: RelationType = { key: '', label: '', description: '', blocking: false };

function strip<T>({ uid: _uid, isNew: _isNew, ...rest }: Draft<T>): T {
  return rest as T;
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <Switch checked={checked} onCheckedChange={onChange} />
      {label}
    </label>
  );
}

function OntologyEditor({ graph, onDone }: { graph: Graph; onDone: () => void }) {
  const [nodeTypes, setNodeTypes] = useState(() =>
    graph.ontology.node_types.map((t) => draft(t, false)),
  );
  const [relationTypes, setRelationTypes] = useState(() =>
    graph.ontology.relation_types.map((t) => draft(t, false)),
  );
  const { mutate, isPending, error } = useReplaceOntology(graph.id);

  const nodeUse = (key: string) => graph.nodes.filter((n) => n.kind === key).length;
  const edgeUse = (key: string) => graph.edges.filter((e) => e.kind === key).length;
  const patchNode = (uid: string, patch: Partial<NodeType>) =>
    setNodeTypes((list) => list.map((t) => (t.uid === uid ? { ...t, ...patch } : t)));
  const patchRelation = (uid: string, patch: Partial<RelationType>) =>
    setRelationTypes((list) => list.map((t) => (t.uid === uid ? { ...t, ...patch } : t)));

  const save = () => {
    // New types get their key from the label; the server turns it into a slug.
    const withKey = <T extends { key: string; label: string }>(t: Draft<T>): T => {
      const value = strip(t);
      return t.isNew ? { ...value, key: value.label } : value;
    };
    const ontology: Ontology = {
      node_types: nodeTypes.filter((t) => t.label.trim() || !t.isNew).map(withKey),
      relation_types: relationTypes.filter((t) => t.label.trim() || !t.isNew).map(withKey),
    };
    mutate(ontology, { onSuccess: onDone });
  };

  return (
    <>
      <div className="max-h-[60svh] space-y-6 overflow-y-auto pr-1">
        <section className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold">Node types</h3>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setNodeTypes((list) => [...list, draft(NEW_NODE_TYPE, true)])}
            >
              <PlusIcon />
              Add type
            </Button>
          </div>
          <ul className="space-y-2">
            {nodeTypes.map((t) => {
              const used = t.isNew ? 0 : nodeUse(t.key);
              return (
                <li key={t.uid} className="space-y-2 rounded-lg border p-2.5">
                  <div className="flex items-center gap-2">
                    <input
                      type="color"
                      value={t.color || '#6366f1'}
                      onChange={(e) => patchNode(t.uid, { color: e.target.value })}
                      aria-label="Colour"
                      className="size-8 shrink-0 cursor-pointer rounded border bg-transparent p-0.5"
                    />
                    <Input
                      value={t.label}
                      maxLength={60}
                      placeholder="Label, e.g. Market signal"
                      aria-label="Label"
                      onChange={(e) => patchNode(t.uid, { label: e.target.value })}
                      className="h-8"
                    />
                    <select
                      value={t.icon}
                      aria-label="Icon"
                      onChange={(e) => patchNode(t.uid, { icon: e.target.value })}
                      className="h-8 shrink-0 rounded-md border bg-background px-2 text-sm"
                    >
                      {NODE_TYPE_ICON_NAMES.map((name) => (
                        <option key={name} value={name}>
                          {name}
                        </option>
                      ))}
                    </select>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="shrink-0 text-destructive"
                      disabled={used > 0}
                      aria-label={`Remove ${t.label || 'type'}`}
                      title={used > 0 ? `Used by ${used} node(s)` : 'Remove'}
                      onClick={() => setNodeTypes((list) => list.filter((x) => x.uid !== t.uid))}
                    >
                      <Trash2Icon />
                    </Button>
                  </div>
                  <Input
                    value={t.description}
                    maxLength={500}
                    placeholder="What a node of this type means (shown to the planner and agents)"
                    aria-label="Description"
                    onChange={(e) => patchNode(t.uid, { description: e.target.value })}
                    className="h-8"
                  />
                  <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                    <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
                      Role
                      <Input
                        value={t.default_role}
                        maxLength={64}
                        placeholder="engineer"
                        onChange={(e) => patchNode(t.uid, { default_role: e.target.value })}
                        className="h-7 w-28 text-xs"
                      />
                    </label>
                    <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
                      Stage
                      <Input
                        type="number"
                        value={t.stage}
                        onChange={(e) => patchNode(t.uid, { stage: Number(e.target.value) || 0 })}
                        className="h-7 w-16 text-xs"
                      />
                    </label>
                    <Toggle
                      label="Delivers a file"
                      checked={t.produces_artifact}
                      onChange={(produces_artifact) => patchNode(t.uid, { produces_artifact })}
                    />
                    <Toggle
                      label="May run code"
                      checked={t.allow_code_exec}
                      onChange={(allow_code_exec) => patchNode(t.uid, { allow_code_exec })}
                    />
                    <span className="ml-auto font-mono text-[11px] text-muted-foreground">
                      {t.isNew ? 'new' : `${t.key} · ${used} node(s)`}
                    </span>
                  </div>
                </li>
              );
            })}
          </ul>
        </section>

        <section className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold">Relation types</h3>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setRelationTypes((list) => [...list, draft(NEW_RELATION_TYPE, true)])}
            >
              <PlusIcon />
              Add relation
            </Button>
          </div>
          <ul className="space-y-2">
            {relationTypes.map((t) => {
              const used = t.isNew ? 0 : edgeUse(t.key);
              return (
                <li key={t.uid} className="space-y-2 rounded-lg border p-2.5">
                  <div className="flex items-center gap-2">
                    <Input
                      value={t.label}
                      maxLength={60}
                      placeholder="Label, e.g. Confirms"
                      aria-label="Label"
                      onChange={(e) => patchRelation(t.uid, { label: e.target.value })}
                      className="h-8"
                    />
                    <Toggle
                      label="Blocking"
                      checked={t.blocking}
                      onChange={(blocking) => patchRelation(t.uid, { blocking })}
                    />
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="shrink-0 text-destructive"
                      disabled={used > 0}
                      aria-label={`Remove ${t.label || 'relation'}`}
                      title={used > 0 ? `Used by ${used} edge(s)` : 'Remove'}
                      onClick={() =>
                        setRelationTypes((list) => list.filter((x) => x.uid !== t.uid))
                      }
                    >
                      <Trash2Icon />
                    </Button>
                  </div>
                  <div className="flex items-center gap-2">
                    <Input
                      value={t.description}
                      maxLength={500}
                      placeholder="What the relation asserts about source and target"
                      aria-label="Description"
                      onChange={(e) => patchRelation(t.uid, { description: e.target.value })}
                      className="h-8"
                    />
                    <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                      {t.isNew ? 'new' : `${t.key} · ${used} edge(s)`}
                    </span>
                  </div>
                </li>
              );
            })}
          </ul>
          <p className="text-xs text-muted-foreground">
            A blocking relation orders execution: its source must finish before its target runs.
            Other relations only record meaning.
          </p>
        </section>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(error)}
        </p>
      )}
      <DialogFooter>
        <Button variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <AnimatedButton onClick={save} loading={isPending} loadingText="Saving…">
          Save ontology
        </AnimatedButton>
      </DialogFooter>
    </>
  );
}

/** Toolbar entry to view and edit the graph's node types and relation types. */
export function OntologyDialog({ graph }: { graph: Graph }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <ToolbarButton icon={ShapesIcon} label="Ontology" showLabel onClick={() => setOpen(true)} />
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>Ontology</DialogTitle>
            <DialogDescription>
              The kinds of node and relation this graph is built from. The planner reuses these and
              proposes new ones when the goal needs them.
            </DialogDescription>
          </DialogHeader>
          {open && <OntologyEditor graph={graph} onDone={() => setOpen(false)} />}
        </DialogContent>
      </Dialog>
    </>
  );
}
