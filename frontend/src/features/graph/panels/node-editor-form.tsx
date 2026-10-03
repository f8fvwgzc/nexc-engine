import { zodResolver } from '@hookform/resolvers/zod';
import { useQuery } from '@tanstack/react-query';
import { SaveIcon, Trash2Icon } from 'lucide-react';
import { useId } from 'react';
import { useForm } from 'react-hook-form';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { FormField } from '@/components/custom-ui/form-field';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { Button } from '@/components/ui/button';
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { agentsQuery } from '@/features/agents/api';
import { applyProblemToForm } from '@/lib/api/form-errors';
import { nodeFormSchema, parseTags, type GraphNode, type NodeFormValues } from '@/schemas/graph';

import { useUpdateNode } from '../hooks/use-graph-mutations';
import { EXECUTOR_OPTIONS, KIND_OPTIONS } from './options';

function toFormValues(node: GraphNode): NodeFormValues {
  return {
    title: node.title,
    content: node.content,
    kind: node.kind,
    executor: node.executor,
    agent_role: node.agent_role ?? '',
    tags: node.tags.join(', '),
  };
}

const FIELDS = ['title', 'content', 'kind', 'executor', 'agent_role', 'tags'] as const;

export function NodeEditorForm({ node, onDelete }: { node: GraphNode; onDelete: () => void }) {
  const rolesListId = useId();
  const updateNode = useUpdateNode(node.graph_id);
  const { data: roles = [] } = useQuery({
    ...agentsQuery(),
    select: (agents) => [...new Set(agents.map((a) => a.role))],
  });
  const form = useForm<NodeFormValues>({
    resolver: zodResolver(nodeFormSchema),
    defaultValues: toFormValues(node),
  });

  const submit = form.handleSubmit((values) =>
    updateNode.mutate(
      {
        nodeId: node.id,
        body: { ...values, agent_role: values.agent_role || null, tags: parseTags(values.tags) },
      },
      {
        onSuccess: (saved) => form.reset(toFormValues(saved)),
        onError: (error) => applyProblemToForm(error, form.setError, FIELDS),
      },
    ),
  );

  return (
    <form noValidate onSubmit={(e) => void submit(e)} className="space-y-5">
      <FieldGroup className="gap-4">
        <FormField control={form.control} name="title" label="Title">
          {(field) => <Input {...field} maxLength={200} />}
        </FormField>
        <div className="grid grid-cols-2 gap-3">
          <FormField control={form.control} name="kind" label="Kind">
            {({ value, onChange, ...field }) => (
              <OptionSelect
                {...field}
                value={value}
                onValueChange={onChange}
                options={KIND_OPTIONS}
              />
            )}
          </FormField>
          <FormField control={form.control} name="executor" label="Executor">
            {({ value, onChange, ...field }) => (
              <OptionSelect
                {...field}
                value={value}
                onValueChange={onChange}
                options={EXECUTOR_OPTIONS}
              />
            )}
          </FormField>
        </div>
        <FormField
          control={form.control}
          name="agent_role"
          label="Agent role"
          description="Which agent executes this node (e.g. researcher, writer). Optional."
        >
          {(field) => <Input {...field} list={rolesListId} placeholder="researcher" />}
        </FormField>
        <datalist id={rolesListId}>
          {roles.map((role) => (
            <option key={role} value={role} />
          ))}
        </datalist>
        <FormField control={form.control} name="tags" label="Tags" description="Comma separated.">
          {(field) => <Input {...field} placeholder="draft, chapter-1" />}
        </FormField>
        <FormField
          control={form.control}
          name="content"
          label="Content"
          description={
            <>
              Markdown. Link other nodes with <code className="font-mono">[[Node title]]</code> —
              dependencies are detected automatically.
            </>
          }
        >
          {(field) => (
            <Textarea
              {...field}
              rows={10}
              className="min-h-40 font-mono text-[13px] leading-relaxed"
            />
          )}
        </FormField>
      </FieldGroup>
      <div className="flex items-center justify-between gap-2">
        <Button type="button" variant="ghost" className="text-destructive" onClick={onDelete}>
          <Trash2Icon />
          Delete
        </Button>
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            disabled={!form.formState.isDirty}
            onClick={() => form.reset(toFormValues(node))}
          >
            Reset
          </Button>
          <AnimatedButton
            type="submit"
            disabled={!form.formState.isDirty}
            loading={updateNode.isPending}
            loadingText="Saving…"
          >
            <SaveIcon />
            Save
          </AnimatedButton>
        </div>
      </div>
    </form>
  );
}
