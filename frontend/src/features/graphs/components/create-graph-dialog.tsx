import { zodResolver } from '@hookform/resolvers/zod';
import { PlusIcon } from 'lucide-react';
import { useForm } from 'react-hook-form';
import { useSearchParams } from 'react-router-dom';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { FormField } from '@/components/custom-ui/form-field';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import { graphInputSchema, type GraphInput } from '@/schemas/graph';

import { useCreateGraph } from '../hooks/use-graph-mutations';

const DEFAULTS: GraphInput = { name: '', description: '', goal: '' };

/** "New graph" dialog. Also opens via `?new=1` (sidebar "+" button). */
export function CreateGraphDialog({ showTrigger = true }: { showTrigger?: boolean }) {
  const [params, setParams] = useSearchParams();
  const open = params.get('new') === '1';
  const setOpen = (next: boolean) =>
    setParams(
      (p) => {
        if (next) p.set('new', '1');
        else p.delete('new');
        return p;
      },
      { replace: true },
    );

  const form = useForm<GraphInput>({
    resolver: zodResolver(graphInputSchema),
    defaultValues: DEFAULTS,
  });
  const createGraph = useCreateGraph();

  const submit = form.handleSubmit((values) =>
    createGraph.mutate(values, {
      onSuccess: () => form.reset(DEFAULTS),
      onError: (error) => {
        if (!applyProblemToForm(error, form.setError, ['name', 'description', 'goal'])) {
          form.setError('root.server', { message: errorMessage(error) });
        }
      },
    }),
  );

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {showTrigger && (
        <DialogTrigger asChild>
          <Button>
            <PlusIcon />
            New graph
          </Button>
        </DialogTrigger>
      )}
      <DialogContent className="sm:max-w-lg">
        <form noValidate onSubmit={(e) => void submit(e)} className="space-y-6">
          <DialogHeader>
            <DialogTitle>New graph</DialogTitle>
            <DialogDescription>
              A graph is a set of topics and tasks that build toward one goal.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup className="gap-4">
            {form.formState.errors.root?.server && (
              <p role="alert" className="text-sm text-destructive">
                {form.formState.errors.root.server.message}
              </p>
            )}
            <FormField control={form.control} name="name" label="Name">
              {(field) => (
                <Input
                  {...field}
                  placeholder="Research report on solid-state batteries"
                  autoFocus
                />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="goal"
              label="Goal"
              description="What should the finished run produce? The planner and every node see this."
            >
              {(field) => (
                <Textarea {...field} rows={3} placeholder="A 10-page .docx report with citations" />
              )}
            </FormField>
            <FormField control={form.control} name="description" label="Description">
              {(field) => <Textarea {...field} rows={2} placeholder="Optional notes" />}
            </FormField>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <AnimatedButton
              type="submit"
              glow
              loading={createGraph.isPending}
              loadingText="Creating…"
            >
              Create graph
            </AnimatedButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
