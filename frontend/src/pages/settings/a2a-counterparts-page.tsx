import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { HandshakeIcon, PlusIcon, RefreshCwIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { CopyButton } from '@/components/custom-ui/copy-button';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { FormField } from '@/components/custom-ui/form-field';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { IconButton } from '@/components/custom-ui/icon-button';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Skeleton } from '@/components/ui/skeleton';
import {
  a2aCounterpartsQuery,
  checkA2aCounterpart,
  registerA2aCounterpart,
  removeA2aCounterpart,
} from '@/features/a2a/api';
import { agentsQuery } from '@/features/agents/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import {
  EMPTY_A2A_COUNTERPART,
  a2aCounterpartInputSchema,
  cardName,
  cardStreams,
  type A2aCounterpart,
  type A2aCounterpartInput,
  type RegisteredA2aCounterpart,
} from '@/schemas/a2a';
import { DATA_LABELS, type Agent } from '@/schemas/agent';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

/** The token of a counterpart just registered: shown until dismissed, never again. */
function IssuedToken({
  registered,
  onDone,
}: {
  registered: RegisteredA2aCounterpart;
  onDone: () => void;
}) {
  return (
    <div
      role="status"
      className="space-y-2 rounded-lg border border-[var(--status-succeeded)]/40 bg-[var(--status-succeeded)]/10 p-3"
    >
      <p className="text-xs font-medium">
        {registered.counterpart.name} registered. Hand this token to the remote agent: it is what
        its calls to your exposed agents present, and it is not shown again. More can be issued on
        the agent card of <code className="font-mono">{registered.counterpart.principal_id}</code>.
      </p>
      <div className="flex items-center gap-1">
        <code
          className="min-w-0 flex-1 truncate rounded bg-background px-2 py-1 font-mono text-xs"
          aria-label="Token"
        >
          {registered.token.secret}
        </code>
        <CopyButton value={registered.token.secret} label="Copy token" />
      </div>
      <Button size="xs" variant="outline" onClick={onDone}>
        Done
      </Button>
    </div>
  );
}

function RegisterDialog({
  open,
  onOpenChange,
  workspaceId,
  agents,
  onRegistered,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceId: string;
  agents: Agent[];
  onRegistered: (registered: RegisteredA2aCounterpart) => void;
}) {
  const queryClient = useQueryClient();
  const form = useForm<A2aCounterpartInput>({
    resolver: zodResolver(a2aCounterpartInputSchema),
    defaultValues: EMPTY_A2A_COUNTERPART,
  });
  const register = useMutation({
    mutationFn: (input: A2aCounterpartInput) => registerA2aCounterpart(workspaceId, input),
    onSuccess: (registered) => {
      const { counterpart } = registered;
      toast.success(
        counterpart.last_check_ok
          ? `${counterpart.name} registered: ${counterpart.last_check_detail}`
          : `${counterpart.name} registered, but its card did not answer: ${counterpart.last_check_detail}`,
      );
      void queryClient.invalidateQueries({ queryKey: qk.a2a.list(workspaceId) });
      void queryClient.invalidateQueries({ queryKey: qk.agents.all });
      form.reset(EMPTY_A2A_COUNTERPART);
      onOpenChange(false);
      onRegistered(registered);
    },
    onError: (error) => {
      const fields = ['name', 'card_url', 'auth_header', 'allowed_agents', 'max_label'] as const;
      if (!applyProblemToForm(error, form.setError, fields)) toast.error(errorMessage(error));
    },
  });
  const callers = agents.filter((a) => a.runtime !== 'external');
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <form onSubmit={form.handleSubmit((input) => register.mutate(input))} noValidate>
          <DialogHeader>
            <DialogTitle>Register an A2A counterpart</DialogTitle>
            <DialogDescription>
              A remote agent your agents may call as <code>a2a:&lt;name&gt;</code>, and that may
              send your exposed agents tasks with the token issued here. Its card is fetched at
              once.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup className="py-4">
            <FormField control={form.control} name="name" label="Name">
              {(field) => <Input {...field} placeholder="partner" autoComplete="off" />}
            </FormField>
            <FormField
              control={form.control}
              name="card_url"
              label="Agent card URL"
              description="Its host is checked like any egress: no private or loopback host unless the server allows it, nothing the workspace policy denies."
            >
              {(field) => (
                <Input
                  {...field}
                  placeholder="https://agent.partner.example/.well-known/agent.json"
                  autoComplete="off"
                />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="auth_header"
              label="Authorization header"
              description="Sent with every task; kept encrypted and never shown again. Empty sends none."
            >
              {(field) => (
                <Input {...field} type="password" placeholder="Bearer …" autoComplete="off" />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="allowed_agents"
              label="Agents that may call it"
              description="None ticked lets any agent the policy allows."
            >
              {({ value, onChange, id }) => (
                <div id={id} className="flex flex-wrap gap-x-4 gap-y-2" role="group">
                  {callers.length === 0 && (
                    <span className="text-xs text-muted-foreground">No agents yet.</span>
                  )}
                  {callers.map((a) => {
                    const checked = value.includes(a.id);
                    return (
                      <Label key={a.id} className="flex items-center gap-2 text-sm font-normal">
                        <Checkbox
                          checked={checked}
                          onCheckedChange={(next) =>
                            onChange(
                              next === true
                                ? [...value, a.id]
                                : value.filter((id: string) => id !== a.id),
                            )
                          }
                        />
                        {a.name}
                      </Label>
                    );
                  })}
                </div>
              )}
            </FormField>
            <FormField
              control={form.control}
              name="max_label"
              label="Data clearance"
              description="The strictest classification of data a node may have read and still send it a task."
            >
              {({ value, onChange, ...field }) => (
                <OptionSelect
                  {...field}
                  value={value}
                  onValueChange={onChange}
                  options={DATA_LABELS.map((label) => ({ value: label, label }))}
                />
              )}
            </FormField>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <AnimatedButton type="submit" loading={register.isPending} loadingText="Fetching…">
              Register and fetch the card
            </AnimatedButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function CounterpartRow({
  counterpart,
  workspace,
  agents,
}: {
  counterpart: A2aCounterpart;
  workspace: Workspace;
  agents: Agent[];
}) {
  const queryClient = useQueryClient();
  const [removing, setRemoving] = useState(false);
  const admin = isWorkspaceAdmin(workspace.role);
  const invalidate = () => queryClient.invalidateQueries({ queryKey: qk.a2a.list(workspace.id) });
  const check = useMutation({
    mutationFn: () => checkA2aCounterpart(workspace.id, counterpart.id),
    onSuccess: (found) => {
      toast[found.ok ? 'success' : 'error'](`${counterpart.name}: ${found.detail}`);
      void invalidate();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const remove = useMutation({
    mutationFn: () => removeA2aCounterpart(workspace.id, counterpart.id),
    onSuccess: () => {
      toast.success(`${counterpart.name} removed`);
      void invalidate();
      void queryClient.invalidateQueries({ queryKey: qk.agents.all });
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const names = new Map(agents.map((a) => [a.id, a.name]));
  const callers =
    counterpart.allowed_agents.length === 0
      ? 'any agent the policy allows'
      : counterpart.allowed_agents.map((id) => names.get(id) ?? id.slice(0, 8)).join(', ');
  const remote = cardName(counterpart);
  return (
    <li className="flex flex-col gap-2 px-3 py-3 text-[13px] sm:flex-row sm:items-start sm:gap-4">
      <div className="min-w-0 flex-1 space-y-1">
        <p className="flex flex-wrap items-center gap-2">
          <span className="font-medium">{counterpart.name}</span>
          <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">
            a2a:{counterpart.name}
          </code>
          <Badge variant="outline">{counterpart.max_label} at most</Badge>
          {counterpart.last_check_ok === null ? (
            <Badge variant="secondary">not checked</Badge>
          ) : (
            <Badge variant={counterpart.last_check_ok ? 'outline' : 'destructive'}>
              {counterpart.last_check_ok ? 'card read' : 'card not read'}
            </Badge>
          )}
          {cardStreams(counterpart) && <Badge variant="secondary">streams</Badge>}
          {counterpart.has_auth_header && <Badge variant="secondary">header set</Badge>}
        </p>
        <p
          className="truncate font-mono text-xs text-muted-foreground"
          title={counterpart.card_url}
        >
          {counterpart.card_url}
        </p>
        <p className="text-xs text-muted-foreground">
          {remote ? `${remote} · ` : ''}
          callers: {callers} · its calls act as{' '}
          <span className="font-mono">{counterpart.principal_id}</span>
          {counterpart.last_checked_at &&
            ` · checked ${formatRelative(counterpart.last_checked_at)}`}
          {counterpart.last_check_detail && ` · ${counterpart.last_check_detail}`}
        </p>
      </div>
      {admin && (
        <div className="flex shrink-0 gap-1">
          <Button
            size="sm"
            variant="outline"
            onClick={() => check.mutate()}
            disabled={check.isPending}
            aria-label={`Check ${counterpart.name}`}
          >
            <RefreshCwIcon />
            Check
          </Button>
          <IconButton label={`Remove ${counterpart.name}`} onClick={() => setRemoving(true)}>
            <Trash2Icon />
          </IconButton>
          <ConfirmDialog
            open={removing}
            onOpenChange={setRemoving}
            title={`Remove ${counterpart.name}?`}
            description="Its principal goes with it, so its tokens stop working at once; its tool stops being offered at the next run. Spans that name it stay on the record."
            confirmLabel="Remove"
            destructive
            onConfirm={() => remove.mutate()}
          />
        </div>
      )}
    </li>
  );
}

function Counterparts({
  workspace,
  adding,
  setAdding,
}: {
  workspace: Workspace;
  adding: boolean;
  setAdding: (adding: boolean) => void;
}) {
  const {
    data: counterparts,
    isPending,
    error,
    refetch,
  } = useQuery(a2aCounterpartsQuery(workspace.id));
  const { data: agents } = useQuery(agentsQuery(workspace.id));
  const [registered, setRegistered] = useState<RegisteredA2aCounterpart | null>(null);
  const admin = isWorkspaceAdmin(workspace.role);
  if (isPending) return <Skeleton className="h-40 rounded-xl" />;
  if (error) {
    return (
      <ErrorState
        title="Couldn’t load the counterparts"
        error={error}
        onRetry={() => void refetch()}
      />
    );
  }
  return (
    <div className="space-y-3">
      {counterparts.length > 0 && (
        <p className="text-[13px] text-muted-foreground">
          {`${counterparts.length} counterpart${counterparts.length === 1 ? '' : 's'}; agents send them tasks through the gateway, under the policy, and they send tasks back with the tokens issued here.`}
        </p>
      )}
      {registered && <IssuedToken registered={registered} onDone={() => setRegistered(null)} />}
      {counterparts.length === 0 ? (
        <EmptyState
          icon={HandshakeIcon}
          title="No A2A counterparts"
          description="Register a remote agent by its card URL and it appears to agents as a2a:<name>; every task out is decided by the policy, screened for data leaving and recorded as a tool_call and an egress span, and every task in carries the token issued at registration."
          action={
            admin ? <Button onClick={() => setAdding(true)}>Register the first</Button> : undefined
          }
        />
      ) : (
        <ol className="divide-y rounded-lg border" aria-label="A2A counterparts">
          {counterparts.map((counterpart) => (
            <CounterpartRow
              key={counterpart.id}
              counterpart={counterpart}
              workspace={workspace}
              agents={agents ?? []}
            />
          ))}
        </ol>
      )}
      {admin && (
        <RegisterDialog
          open={adding}
          onOpenChange={setAdding}
          workspaceId={workspace.id}
          agents={agents ?? []}
          onRegistered={setRegistered}
        />
      )}
    </div>
  );
}

export default function A2aCounterpartsSettingsPage() {
  const { current } = useCurrentWorkspace();
  const [adding, setAdding] = useState(false);
  if (!current) return <PageSkeleton width="narrow" />;
  return (
    <PageContainer width="narrow">
      <Seo title="A2A counterparts" noIndex />
      <PageHeader
        title="A2A counterparts"
        description={`Remote agents ${current.name} exchanges tasks with over the Agent-to-Agent protocol: your agents call them as tools through the gateway, and they call your published agents with a token issued here. Every task either way is on the trace.`}
        actions={
          isWorkspaceAdmin(current.role) && (
            <Button onClick={() => setAdding(true)}>
              <PlusIcon />
              Register a counterpart
            </Button>
          )
        }
      />
      <Counterparts key={current.id} workspace={current} adding={adding} setAdding={setAdding} />
    </PageContainer>
  );
}
