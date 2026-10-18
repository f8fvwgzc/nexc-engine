import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { PlugZapIcon, PlusIcon, RefreshCwIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';
import { useForm, useWatch } from 'react-hook-form';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { FormField } from '@/components/custom-ui/form-field';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
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
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Textarea } from '@/components/ui/textarea';
import {
  checkMcpServer,
  mcpServersQuery,
  registerMcpServer,
  removeMcpServer,
} from '@/features/gateway/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { applyProblemToForm } from '@/lib/api/form-errors';
import { formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { DATA_LABELS } from '@/schemas/agent';
import {
  EMPTY_MCP_SERVER,
  mcpServerInputSchema,
  type McpServer,
  type McpServerInput,
} from '@/schemas/mcp';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

function RegisterDialog({
  open,
  onOpenChange,
  workspaceId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceId: string;
}) {
  const queryClient = useQueryClient();
  const form = useForm<McpServerInput>({
    resolver: zodResolver(mcpServerInputSchema),
    defaultValues: EMPTY_MCP_SERVER,
  });
  const transport = useWatch({ control: form.control, name: 'transport' });
  const register = useMutation({
    mutationFn: (input: McpServerInput) => registerMcpServer(workspaceId, input),
    onSuccess: (server) => {
      toast.success(
        server.last_check_ok
          ? `${server.name} registered: ${server.tools.length} tool(s) listed`
          : `${server.name} registered, but it did not answer: ${server.last_check_detail}`,
      );
      void queryClient.invalidateQueries({ queryKey: qk.mcp.list(workspaceId) });
      form.reset(EMPTY_MCP_SERVER);
      onOpenChange(false);
    },
    onError: (error) => {
      const fields = [
        'name',
        'command',
        'args',
        'url',
        'auth_header',
        'allowed_tools',
        'max_label',
      ] as const;
      if (!applyProblemToForm(error, form.setError, fields)) toast.error(errorMessage(error));
    },
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <form onSubmit={form.handleSubmit((input) => register.mutate(input))} noValidate>
          <DialogHeader>
            <DialogTitle>Register an MCP server</DialogTitle>
            <DialogDescription>
              The backend connects to it on an agent&apos;s behalf and offers its tools as{' '}
              <code>mcp:&lt;name&gt;/&lt;tool&gt;</code> under the policy. The runtime never talks
              to it.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup className="py-4">
            <FormField control={form.control} name="name" label="Name">
              {(field) => <Input {...field} placeholder="github" autoComplete="off" />}
            </FormField>
            <FormField control={form.control} name="transport" label="Transport">
              {({ value, onChange, ...field }) => (
                <OptionSelect
                  {...field}
                  value={value}
                  onValueChange={onChange}
                  options={[
                    { value: 'stdio', label: 'Child process (stdio)' },
                    { value: 'http', label: 'Streamable HTTP' },
                  ]}
                />
              )}
            </FormField>
            {transport === 'stdio' ? (
              <>
                <FormField
                  control={form.control}
                  name="command"
                  label="Command"
                  description="Started for every call with an empty environment, in a scratch directory."
                >
                  {(field) => <Input {...field} placeholder="python3" autoComplete="off" />}
                </FormField>
                <FormField
                  control={form.control}
                  name="args"
                  label="Arguments"
                  description="One per line."
                >
                  {(field) => (
                    <Textarea {...field} rows={3} placeholder={'-I\n/srv/mcp/server.py'} />
                  )}
                </FormField>
              </>
            ) : (
              <>
                <FormField control={form.control} name="url" label="URL">
                  {(field) => (
                    <Input
                      {...field}
                      placeholder="https://mcp.example.com/mcp"
                      autoComplete="off"
                    />
                  )}
                </FormField>
                <FormField
                  control={form.control}
                  name="auth_header"
                  label="Authorization header"
                  description="Kept encrypted and never shown again; empty sends none."
                >
                  {(field) => (
                    <Input {...field} type="password" placeholder="Bearer …" autoComplete="off" />
                  )}
                </FormField>
              </>
            )}
            <FormField
              control={form.control}
              name="allowed_tools"
              label="Allowed tools"
              description="Tool names, one per line; empty offers every tool the server lists."
            >
              {(field) => <Textarea {...field} rows={2} />}
            </FormField>
            <FormField
              control={form.control}
              name="max_label"
              label="Data clearance"
              description="The strictest classification of data a node may have read and still call this server."
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
            <AnimatedButton type="submit" loading={register.isPending} loadingText="Connecting…">
              Register and connect
            </AnimatedButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ServerRow({ server, workspace }: { server: McpServer; workspace: Workspace }) {
  const queryClient = useQueryClient();
  const [removing, setRemoving] = useState(false);
  const admin = isWorkspaceAdmin(workspace.role);
  const invalidate = () => queryClient.invalidateQueries({ queryKey: qk.mcp.list(workspace.id) });
  const check = useMutation({
    mutationFn: () => checkMcpServer(workspace.id, server.id),
    onSuccess: (found) => {
      toast[found.ok ? 'success' : 'error'](`${server.name}: ${found.detail}`);
      void invalidate();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const remove = useMutation({
    mutationFn: () => removeMcpServer(workspace.id, server.id),
    onSuccess: () => {
      toast.success(`${server.name} removed`);
      void invalidate();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const where =
    server.transport === 'http' ? server.url : [server.command, ...server.args].join(' ');
  return (
    <li className="flex flex-col gap-2 px-3 py-3 text-[13px] sm:flex-row sm:items-start sm:gap-4">
      <div className="min-w-0 flex-1 space-y-1">
        <p className="flex flex-wrap items-center gap-2">
          <span className="font-medium">{server.name}</span>
          <Badge variant="outline">{server.transport}</Badge>
          <Badge variant="outline">{server.max_label} at most</Badge>
          {server.last_check_ok === null ? (
            <Badge variant="secondary">not checked</Badge>
          ) : (
            <Badge variant={server.last_check_ok ? 'outline' : 'destructive'}>
              {server.last_check_ok ? 'answers' : 'not answering'}
            </Badge>
          )}
          {server.has_auth_header && <Badge variant="secondary">header set</Badge>}
        </p>
        <p className="truncate font-mono text-xs text-muted-foreground" title={where ?? ''}>
          {where}
        </p>
        <p className="text-xs text-muted-foreground">
          {server.tools.length === 0
            ? 'No tools listed yet.'
            : `Tools: ${server.tools
                .map((t) =>
                  server.allowed_tools.length === 0 || server.allowed_tools.includes(t.name)
                    ? `mcp:${server.name}/${t.name}`
                    : null,
                )
                .filter(Boolean)
                .join(', ')}`}
          {server.last_checked_at && ` · checked ${formatRelative(server.last_checked_at)}`}
          {server.last_check_detail && ` · ${server.last_check_detail}`}
        </p>
      </div>
      {admin && (
        <div className="flex shrink-0 gap-1">
          <Button
            size="sm"
            variant="outline"
            onClick={() => check.mutate()}
            disabled={check.isPending}
            aria-label={`Check ${server.name}`}
          >
            <RefreshCwIcon />
            Check
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setRemoving(true)}
            aria-label={`Remove ${server.name}`}
          >
            <Trash2Icon />
          </Button>
          <ConfirmDialog
            open={removing}
            onOpenChange={setRemoving}
            title={`Remove ${server.name}?`}
            description="Its tools stop being offered at the next run. Spans that name it stay on the record."
            confirmLabel="Remove"
            destructive
            onConfirm={() => remove.mutate()}
          />
        </div>
      )}
    </li>
  );
}

function Servers({ workspace }: { workspace: Workspace }) {
  const { data: servers, isPending, error } = useQuery(mcpServersQuery(workspace.id));
  const [adding, setAdding] = useState(false);
  const admin = isWorkspaceAdmin(workspace.role);
  if (isPending) return <Skeleton className="h-40 rounded-xl" />;
  if (error) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-[13px] text-muted-foreground">
          {servers.length === 0
            ? 'No servers registered.'
            : `${servers.length} server${servers.length === 1 ? '' : 's'}; agents call their tools through the gateway, under the policy.`}
        </p>
        {admin && (
          <Button size="sm" onClick={() => setAdding(true)}>
            <PlusIcon />
            Register a server
          </Button>
        )}
      </div>
      {servers.length === 0 ? (
        <EmptyState
          icon={PlugZapIcon}
          title="No MCP servers"
          description="Register a server and its tools appear to agents as mcp:<name>/<tool>; every call is decided by the policy, screened for data leaving, rate-limited and recorded as a tool_call and an egress span."
          action={
            admin ? <Button onClick={() => setAdding(true)}>Register the first</Button> : undefined
          }
        />
      ) : (
        <ol className="divide-y rounded-lg border" aria-label="MCP servers">
          {servers.map((server) => (
            <ServerRow key={server.id} server={server} workspace={workspace} />
          ))}
        </ol>
      )}
      {admin && (
        <RegisterDialog open={adding} onOpenChange={setAdding} workspaceId={workspace.id} />
      )}
    </div>
  );
}

export default function McpServersSettingsPage() {
  const { current } = useCurrentWorkspace();
  if (!current) return <PageSkeleton />;
  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <Seo title="MCP servers" noIndex />
      <PageHeader
        title="MCP servers"
        description={`The tools gateway of ${current.name}: servers the backend calls on an agent's behalf. The agent runtime never reaches them; a call goes up the stream, is decided here and comes back as a result.`}
      />
      <Servers workspace={current} />
    </div>
  );
}
