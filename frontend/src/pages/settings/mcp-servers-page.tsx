import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { PlugZapIcon, PlusIcon, RefreshCwIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
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
      const fields = ['name', 'url', 'auth_header', 'allowed_tools', 'max_label'] as const;
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
              The backend connects to it over streamable HTTP on an agent&apos;s behalf and offers
              its tools as <code>mcp:&lt;name&gt;/&lt;tool&gt;</code> under the policy. The runtime
              never talks to it, and the backend starts no command: run a stdio server in a sandbox
              of its own behind an HTTP bridge and register the bridge&apos;s URL.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup className="py-4">
            <FormField control={form.control} name="name" label="Name">
              {(field) => <Input {...field} placeholder="github" autoComplete="off" />}
            </FormField>
            <FormField control={form.control} name="url" label="URL">
              {(field) => (
                <Input {...field} placeholder="https://mcp.example.com/mcp" autoComplete="off" />
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
          {server.transport === 'stdio' ? (
            // Registered before the backend stopped starting commands: listed to be removed.
            <Badge variant="destructive">never started</Badge>
          ) : server.last_check_ok === null ? (
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
          <IconButton label={`Remove ${server.name}`} onClick={() => setRemoving(true)}>
            <Trash2Icon />
          </IconButton>
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

function Servers({
  workspace,
  adding,
  setAdding,
}: {
  workspace: Workspace;
  adding: boolean;
  setAdding: (adding: boolean) => void;
}) {
  const { data: servers, isPending, error, refetch } = useQuery(mcpServersQuery(workspace.id));
  const admin = isWorkspaceAdmin(workspace.role);
  if (isPending) return <Skeleton className="h-40 rounded-xl" />;
  if (error) {
    return (
      <ErrorState title="Couldn’t load the servers" error={error} onRetry={() => void refetch()} />
    );
  }
  return (
    <div className="space-y-3">
      {servers.length > 0 && (
        <p className="text-[13px] text-muted-foreground">
          {`${servers.length} server${servers.length === 1 ? '' : 's'}; agents call their tools through the gateway, under the policy.`}
        </p>
      )}
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
  const [adding, setAdding] = useState(false);
  if (!current) return <PageSkeleton width="narrow" />;
  return (
    <PageContainer width="narrow">
      <Seo title="MCP servers" noIndex />
      <PageHeader
        title="MCP servers"
        description={`The tools gateway of ${current.name}: servers the backend calls on an agent's behalf. The agent runtime never reaches them; a call goes up the stream, is decided here and comes back as a result.`}
        actions={
          isWorkspaceAdmin(current.role) && (
            <Button onClick={() => setAdding(true)}>
              <PlusIcon />
              Register a server
            </Button>
          )
        }
      />
      <Servers key={current.id} workspace={current} adding={adding} setAdding={setAdding} />
    </PageContainer>
  );
}
