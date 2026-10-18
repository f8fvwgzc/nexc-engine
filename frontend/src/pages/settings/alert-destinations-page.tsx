import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { HistoryIcon, PlusIcon, SendIcon, Trash2Icon, WebhookIcon } from 'lucide-react';
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
import { WithTooltip } from '@/components/custom-ui/with-tooltip';
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
import { Switch } from '@/components/ui/switch';
import {
  alertDeliveriesQuery,
  alertDestinationsQuery,
  registerAlertDestination,
  removeAlertDestination,
  testAlertDestination,
  updateAlertDestination,
} from '@/features/alerting/api';
import { ALERT_KIND_LABEL, describeFilters } from '@/features/alerting/describe';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import { formatDateTime, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import {
  EMPTY_ALERT_DESTINATION,
  alertDestinationInputSchema,
  type AlertDelivery,
  type AlertDestination,
  type AlertDestinationInput,
  type DestinationTest,
  type IssuedAlertDestination,
} from '@/schemas/alert-routing';
import { SEVERITY_META, alertKindSchema, severitySchema, type AlertKind } from '@/schemas/policy';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

const DESTINATION_LABEL = { webhook: 'Signed webhook', slack: 'Slack' } as const;

function RegisterDialog({
  open,
  onOpenChange,
  workspaceId,
  onIssued,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceId: string;
  onIssued: (issued: IssuedAlertDestination) => void;
}) {
  const queryClient = useQueryClient();
  const form = useForm<AlertDestinationInput>({
    resolver: zodResolver(alertDestinationInputSchema),
    defaultValues: EMPTY_ALERT_DESTINATION,
  });
  const register = useMutation({
    mutationFn: (input: AlertDestinationInput) => registerAlertDestination(workspaceId, input),
    onSuccess: (issued) => {
      void queryClient.invalidateQueries({ queryKey: qk.alertDestinations.list(workspaceId) });
      form.reset(EMPTY_ALERT_DESTINATION);
      onOpenChange(false);
      onIssued(issued);
    },
    onError: (error) => {
      const fields = ['name', 'kind', 'url', 'kinds', 'min_severity', 'include_approvals'] as const;
      if (!applyProblemToForm(error, form.setError, fields)) toast.error(errorMessage(error));
    },
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <form onSubmit={form.handleSubmit((input) => register.mutate(input))} noValidate>
          <DialogHeader>
            <DialogTitle>Add an alert destination</DialogTitle>
            <DialogDescription>
              Every alert that fits the filters is posted there within seconds, signed with a secret
              shown once. A Slack incoming webhook gets a message; anything else gets the JSON
              event.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup className="py-4">
            <FormField control={form.control} name="name" label="Name">
              {(field) => <Input {...field} placeholder="SIEM" autoComplete="off" />}
            </FormField>
            <FormField control={form.control} name="kind" label="Kind">
              {({ value, onChange, ...field }) => (
                <OptionSelect
                  {...field}
                  value={value}
                  onValueChange={onChange}
                  options={[
                    { value: 'webhook', label: 'Signed JSON webhook' },
                    { value: 'slack', label: 'Slack incoming webhook' },
                  ]}
                />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="url"
              label="URL"
              description="Kept encrypted; only its host is shown again. Hosts on this machine or a private network are refused."
            >
              {(field) => (
                <Input
                  {...field}
                  placeholder="https://siem.example.com/hooks/nexc"
                  autoComplete="off"
                />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="kinds"
              label="Alert kinds"
              description="None ticked sends every kind."
            >
              {({ value, onChange, id }) => (
                <div id={id} className="flex flex-wrap gap-x-4 gap-y-2" role="group">
                  {alertKindSchema.options.map((kind) => (
                    <Label key={kind} className="flex items-center gap-2 text-[13px] font-normal">
                      <Checkbox
                        checked={value.includes(kind)}
                        onCheckedChange={(checked) =>
                          onChange(
                            checked ? [...value, kind] : value.filter((k: AlertKind) => k !== kind),
                          )
                        }
                      />
                      {ALERT_KIND_LABEL[kind]}
                    </Label>
                  ))}
                </div>
              )}
            </FormField>
            <FormField control={form.control} name="min_severity" label="Least severity sent">
              {({ value, onChange, ...field }) => (
                <OptionSelect
                  {...field}
                  value={value}
                  onValueChange={onChange}
                  options={severitySchema.options.map((s) => ({
                    value: s,
                    label: SEVERITY_META[s].label,
                  }))}
                />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="include_approvals"
              label="Approvals"
              description="Also send approvals requested and decided."
            >
              {({ value, onChange, id }) => (
                <Label className="flex items-center gap-2 text-[13px] font-normal">
                  <Checkbox id={id} checked={value} onCheckedChange={(c) => onChange(c === true)} />
                  Send approvals too
                </Label>
              )}
            </FormField>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <AnimatedButton type="submit" loading={register.isPending} loadingText="Adding…">
              Add destination
            </AnimatedButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** The signing secret, shown once after a registration. */
function SecretDialog({
  issued,
  onClose,
}: {
  issued: IssuedAlertDestination | null;
  onClose: () => void;
}) {
  return (
    <Dialog open={issued !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{issued?.destination.name ?? 'Destination'} is set up</DialogTitle>
          <DialogDescription>
            This is the signing secret. It is shown once and never again: every delivery carries{' '}
            <code>X-Nexc-Signature: sha256=&lt;HMAC-SHA256 of the body&gt;</code> under it.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2 rounded-md border bg-muted/40 px-3 py-2">
          <code className="min-w-0 flex-1 font-mono text-xs break-all" data-testid="secret">
            {issued?.secret}
          </code>
          {issued && <CopyButton value={issued.secret} label="Copy the secret" />}
        </div>
        <DialogFooter>
          <Button onClick={onClose}>I have saved it</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const STATUS_VARIANT: Record<AlertDelivery['status'], 'outline' | 'secondary' | 'destructive'> = {
  delivered: 'outline',
  pending: 'secondary',
  failed: 'destructive',
};

function DeliveryLog({
  workspaceId,
  destinationId,
}: {
  workspaceId: string;
  destinationId: string;
}) {
  const { data, isPending, error, refetch } = useQuery(
    alertDeliveriesQuery(workspaceId, destinationId),
  );
  if (isPending) return <Skeleton className="h-16 rounded-lg" />;
  if (error) {
    return (
      <ErrorState
        compact
        title="Couldn’t load the deliveries"
        error={error}
        onRetry={() => void refetch()}
      />
    );
  }
  if (data.length === 0) {
    return <p className="text-xs text-muted-foreground">Nothing sent yet.</p>;
  }
  return (
    <ol className="divide-y rounded-md border text-xs" aria-label="Delivery log">
      {data.map((d) => (
        <li key={d.id} className="flex flex-wrap items-center gap-x-2 gap-y-1 px-2.5 py-1.5">
          <Badge variant={STATUS_VARIANT[d.status]} className="h-5 text-[10px]">
            {d.status}
          </Badge>
          <span className="font-mono">{d.event}</span>
          <span className="text-muted-foreground">
            {d.last_status !== null ? `HTTP ${d.last_status}` : 'no answer'} · {d.attempts} attempt
            {d.attempts === 1 ? '' : 's'} ·{' '}
            <time dateTime={d.created_at} title={formatDateTime(d.created_at)}>
              {formatRelative(d.created_at)}
            </time>
            {d.status === 'pending' && d.next_attempt_at && (
              <> · again {formatRelative(d.next_attempt_at)}</>
            )}
          </span>
          {d.last_error && (
            <span
              className="min-w-0 basis-full truncate text-muted-foreground"
              title={d.last_error}
            >
              {d.last_error}
            </span>
          )}
        </li>
      ))}
    </ol>
  );
}

function DestinationRow({
  destination,
  workspace,
}: {
  destination: AlertDestination;
  workspace: Workspace;
}) {
  const queryClient = useQueryClient();
  const [removing, setRemoving] = useState(false);
  const [logOpen, setLogOpen] = useState(false);
  const [tested, setTested] = useState<DestinationTest | null>(null);
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: qk.alertDestinations.list(workspace.id) });
  const toggle = useMutation({
    mutationFn: (enabled: boolean) =>
      updateAlertDestination(workspace.id, destination.id, { enabled }),
    onSuccess: (next) => {
      toast.success(`${next.name} ${next.enabled ? 'enabled' : 'disabled'}`);
      void invalidate();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const test = useMutation({
    mutationFn: () => testAlertDestination(workspace.id, destination.id),
    onSuccess: (found) => {
      setTested(found);
      void queryClient.invalidateQueries({
        queryKey: qk.alertDestinations.deliveries(workspace.id, destination.id),
      });
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const remove = useMutation({
    mutationFn: () => removeAlertDestination(workspace.id, destination.id),
    onSuccess: () => {
      toast.success(`${destination.name} removed`);
      void invalidate();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  return (
    <li className="flex flex-col gap-2 px-3 py-3 text-[13px]">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:gap-4">
        <div className="min-w-0 flex-1 space-y-1">
          <p className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{destination.name}</span>
            <Badge variant="outline">{DESTINATION_LABEL[destination.kind]}</Badge>
            {destination.enabled ? (
              <Badge variant="outline">on</Badge>
            ) : destination.disabled_reason ? (
              <Badge variant="destructive">switched off</Badge>
            ) : (
              <Badge variant="secondary">off</Badge>
            )}
            {destination.failures > 0 && (
              <Badge variant="secondary">{destination.failures} failed in a row</Badge>
            )}
          </p>
          <p className="truncate font-mono text-xs text-muted-foreground" title={destination.host}>
            {destination.host}
          </p>
          <p className="text-xs text-muted-foreground">
            Sends {describeFilters(destination)}
            {destination.last_delivered_at &&
              ` · last delivered ${formatRelative(destination.last_delivered_at)}`}
          </p>
          {destination.disabled_reason && (
            <p className="text-xs text-destructive">{destination.disabled_reason}</p>
          )}
          {!destination.disabled_reason && destination.last_error && (
            <p className="truncate text-xs text-destructive" title={destination.last_error}>
              {destination.last_error}
            </p>
          )}
          {tested && (
            <p
              role="status"
              className={`text-xs ${tested.ok ? 'text-muted-foreground' : 'text-destructive'}`}
            >
              {tested.ok
                ? `Test delivered: HTTP ${tested.status ?? ''} in ${tested.latency_ms} ms`
                : `Test failed: ${tested.detail}`}
            </p>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <WithTooltip
            label={
              destination.enabled ? 'On: switch off to stop sending' : 'Off: switch on to send'
            }
          >
            <Switch
              size="sm"
              checked={destination.enabled}
              disabled={toggle.isPending}
              onCheckedChange={(enabled) => toggle.mutate(enabled)}
              aria-label={`${destination.enabled ? 'Disable' : 'Enable'} ${destination.name}`}
            />
          </WithTooltip>
          <Button
            size="sm"
            variant="outline"
            onClick={() => test.mutate()}
            disabled={test.isPending}
            aria-label={`Test ${destination.name}`}
          >
            <SendIcon />
            Test
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setLogOpen((open) => !open)}
            aria-expanded={logOpen}
            aria-label={`Delivery log of ${destination.name}`}
          >
            <HistoryIcon />
            Log
          </Button>
          <IconButton label={`Remove ${destination.name}`} onClick={() => setRemoving(true)}>
            <Trash2Icon />
          </IconButton>
          <ConfirmDialog
            open={removing}
            onOpenChange={setRemoving}
            title={`Remove ${destination.name}?`}
            description="Nothing more is sent there, and its delivery log goes with it."
            confirmLabel="Remove"
            destructive
            onConfirm={() => remove.mutate()}
          />
        </div>
      </div>
      {logOpen && <DeliveryLog workspaceId={workspace.id} destinationId={destination.id} />}
    </li>
  );
}

function Destinations({
  workspace,
  adding,
  setAdding,
}: {
  workspace: Workspace;
  adding: boolean;
  setAdding: (adding: boolean) => void;
}) {
  const { data, isPending, error, refetch } = useQuery(alertDestinationsQuery(workspace.id));
  const [issued, setIssued] = useState<IssuedAlertDestination | null>(null);
  if (isPending) return <Skeleton className="h-40 rounded-xl" />;
  if (error) {
    return (
      <ErrorState
        title="Couldn’t load the destinations"
        error={error}
        onRetry={() => void refetch()}
      />
    );
  }
  return (
    <div className="space-y-3">
      {data.length > 0 && (
        <p className="text-[13px] text-muted-foreground">
          {`${data.length} destination${data.length === 1 ? '' : 's'}; a destination that fails 50 times in a row is switched off and an alert says so.`}
        </p>
      )}
      {data.length === 0 ? (
        <EmptyState
          icon={WebhookIcon}
          title="Alerts stay in the console"
          description="Add a signed webhook for your SIEM or a Slack incoming webhook and every alert that fits its filters is delivered within seconds, retried for a day, and logged here. A SIEM that would rather pull can read /alerts/export and /audit/export instead."
          action={<Button onClick={() => setAdding(true)}>Add the first</Button>}
        />
      ) : (
        <ol className="divide-y rounded-lg border" aria-label="Alert destinations">
          {data.map((destination) => (
            <DestinationRow key={destination.id} destination={destination} workspace={workspace} />
          ))}
        </ol>
      )}
      <RegisterDialog
        open={adding}
        onOpenChange={setAdding}
        workspaceId={workspace.id}
        onIssued={setIssued}
      />
      <SecretDialog issued={issued} onClose={() => setIssued(null)} />
    </div>
  );
}

export default function AlertDestinationsSettingsPage() {
  const { current } = useCurrentWorkspace();
  const [adding, setAdding] = useState(false);
  if (!current) return <PageSkeleton width="narrow" />;
  const allowed = isWorkspaceAdmin(current.role);
  return (
    <PageContainer width="narrow">
      <Seo title="Alert destinations" noIndex />
      <PageHeader
        title="Alert destinations"
        description={`Where the alerts of ${current.name} go beyond the console: a signed JSON webhook for a SIEM, a Slack channel. Approvals can go along.`}
        actions={
          allowed && (
            <Button onClick={() => setAdding(true)}>
              <PlusIcon />
              Add a destination
            </Button>
          )
        }
      />
      {allowed ? (
        <Destinations key={current.id} workspace={current} adding={adding} setAdding={setAdding} />
      ) : (
        <EmptyState
          icon={WebhookIcon}
          title="Only admins route alerts"
          description="Ask an owner or admin of this workspace."
        />
      )}
    </PageContainer>
  );
}
