import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { GavelIcon, PlusIcon } from 'lucide-react';
import { useMemo, useState } from 'react';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { ScrollRegion } from '@/components/custom-ui/scroll-region';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Skeleton } from '@/components/ui/skeleton';
import { Textarea } from '@/components/ui/textarea';
import { errorMessage } from '@/lib/api/errors';
import { formatDateTime, formatRelative } from '@/lib/format';
import { cn } from '@/lib/utils';
import {
  EMPTY_POLICY,
  policyDocumentSchema,
  type Policy,
  type PolicyDocument,
  type PolicyScope,
} from '@/schemas/policy';

import { createPolicy, policiesQuery } from '../api';
import { diffLines } from '../diff';

const pretty = (document: PolicyDocument) => JSON.stringify(document, null, 2);

/** Two versions of a policy as a line diff: added lines marked, removed ones struck through. */
export function DocumentDiff({
  before,
  after,
  className,
}: {
  before: Policy | null;
  after: Policy;
  className?: string;
}) {
  const lines = useMemo(
    () => diffLines(before ? pretty(before.document) : null, pretty(after.document)),
    [before, after],
  );
  return (
    <ScrollRegion
      as="pre"
      label={`Version ${after.version} against ${before ? `version ${before.version}` : 'nothing'}`}
      className={cn(
        'max-h-[32rem] overflow-auto rounded-lg border bg-muted/30 p-3 text-xs leading-5',
        className,
      )}
    >
      {lines.map((line, index) => (
        <div
          // Lines repeat; the position is the identity.
          key={index}
          className={cn(
            'whitespace-pre',
            line.kind === 'added' &&
              'bg-[color-mix(in_oklch,var(--status-succeeded)_18%,transparent)]',
            line.kind === 'removed' &&
              'bg-[color-mix(in_oklch,var(--status-failed)_18%,transparent)] line-through opacity-70',
          )}
        >
          <span className="inline-block w-4 text-muted-foreground select-none">
            {line.kind === 'added' ? '+' : line.kind === 'removed' ? '-' : ' '}
          </span>
          {line.text}
        </div>
      ))}
    </ScrollRegion>
  );
}

interface NewVersionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceId: string;
  scope: PolicyScope;
  scopeId: string | null;
  latest: PolicyDocument | null;
}

function NewVersionDialog({
  open,
  onOpenChange,
  workspaceId,
  scope,
  scopeId,
  latest,
}: NewVersionDialogProps) {
  const queryClient = useQueryClient();
  const [text, setText] = useState(() => pretty(latest ?? EMPTY_POLICY));
  const [problem, setProblem] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: (document: PolicyDocument) =>
      createPolicy(workspaceId, {
        scope,
        ...(scopeId ? { scope_id: scopeId } : {}),
        document,
      }),
    onSuccess: (policy) => {
      toast.success(`Policy version ${policy.version} set`);
      void queryClient.invalidateQueries({ queryKey: qkPolicies(workspaceId) });
      onOpenChange(false);
    },
    onError: (error) => setProblem(errorMessage(error)),
  });
  const submit = () => {
    setProblem(null);
    let parsed: unknown;
    try {
      parsed = JSON.parse(text);
    } catch {
      setProblem('The document is not valid JSON.');
      return;
    }
    const result = policyDocumentSchema.safeParse(parsed);
    if (!result.success) {
      setProblem(
        result.error.issues
          .slice(0, 3)
          .map((issue) => `${issue.path.join('.') || 'document'}: ${issue.message}`)
          .join('; '),
      );
      return;
    }
    save.mutate(result.data);
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>New policy version</DialogTitle>
          <DialogDescription>
            The document as JSON. Whatever it leaves at <code>null</code>, <code>[]</code> or{' '}
            <code>{'{}'}</code> is inherited from the level above; a version may only narrow what
            the levels above allow.
          </DialogDescription>
        </DialogHeader>
        <Textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          spellCheck={false}
          className="min-h-[24rem] font-mono text-xs"
          aria-label="Policy document"
        />
        {problem && (
          <p role="alert" className="text-sm text-destructive">
            {problem}
          </p>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <AnimatedButton loading={save.isPending} onClick={submit}>
            Set version
          </AnimatedButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const qkPolicies = (workspaceId: string) => ['policies', workspaceId] as const;

interface PolicyVersionsProps {
  workspaceId: string;
  scope: PolicyScope;
  scopeId: string | null;
  /** What the level is called, for the empty state. */
  levelName: string;
}

/** The history of one level: every version, the diff against the one before, a new version. */
export function PolicyVersions({ workspaceId, scope, scopeId, levelName }: PolicyVersionsProps) {
  const {
    data: versions,
    isPending,
    error,
    refetch,
  } = useQuery(policiesQuery(workspaceId, scope, scopeId ?? undefined));
  const [selected, setSelected] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  if (isPending) return <Skeleton className="h-48 rounded-xl" />;
  if (error) {
    return (
      <ErrorState
        compact
        title={`Couldn’t load the versions of ${levelName}`}
        error={error}
        onRetry={() => void refetch()}
      />
    );
  }
  const current = versions.find((v) => v.version === selected) ?? versions[0] ?? null;
  const before = current ? (versions.find((v) => v.version === current.version - 1) ?? null) : null;
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">
          {versions.length === 0
            ? `No policy set for ${levelName}: everything is inherited.`
            : `${versions.length} version${versions.length === 1 ? '' : 's'}; the latest governs new runs.`}
        </p>
        <Button size="sm" onClick={() => setAdding(true)}>
          <PlusIcon />
          New version
        </Button>
      </div>
      {versions.length === 0 ? (
        <EmptyState
          icon={GavelIcon}
          title={`Nothing set for ${levelName}`}
          description="Add a version to narrow what agents at this level may do: tools, hosts, labels, ceilings, working hours, approvals, alert thresholds, DLP and the injection screen."
          action={<Button onClick={() => setAdding(true)}>Write the first version</Button>}
        />
      ) : (
        <div className="grid gap-3 lg:grid-cols-[14rem_minmax(0,1fr)]">
          <ol className="divide-y rounded-lg border" aria-label="Versions">
            {versions.map((policy) => (
              <li key={policy.id}>
                <button
                  type="button"
                  onClick={() => setSelected(policy.version)}
                  className={cn(
                    'flex w-full flex-col gap-0.5 px-3 py-2 text-left text-xs outline-none hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset',
                    current?.id === policy.id && 'bg-muted/60',
                  )}
                >
                  <span className="flex items-center gap-2">
                    <Badge variant="outline">v{policy.version}</Badge>
                    <span className="truncate font-medium">{policy.author_name || 'Someone'}</span>
                  </span>
                  <time
                    dateTime={policy.created_at}
                    title={formatDateTime(policy.created_at)}
                    className="text-muted-foreground"
                  >
                    {formatRelative(policy.created_at)}
                  </time>
                </button>
              </li>
            ))}
          </ol>
          {current && (
            <div className="min-w-0 space-y-2">
              <p className="text-xs text-muted-foreground">
                Version {current.version}
                {before ? ` against version ${before.version}` : ' (the first)'}: added lines are
                marked, removed ones struck through.
              </p>
              <DocumentDiff before={before} after={current} />
            </div>
          )}
        </div>
      )}
      {adding && (
        <NewVersionDialog
          open={adding}
          onOpenChange={setAdding}
          workspaceId={workspaceId}
          scope={scope}
          scopeId={scopeId}
          latest={versions[0]?.document ?? null}
        />
      )}
    </div>
  );
}
