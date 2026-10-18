import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { KeyRoundIcon, XIcon } from 'lucide-react';
import { useState } from 'react';
import { toast } from 'sonner';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { CopyButton } from '@/components/custom-ui/copy-button';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { formatDateTime, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { TOKEN_NAME_MAX, type AgentApiToken, type IssuedAgentApiToken } from '@/schemas/agent';

import { agentTokensQuery, issueAgentToken, revokeAgentToken } from '../api';

/** The secret of a token just issued: shown until dismissed, never again. */
function IssuedSecret({ issued, onDone }: { issued: IssuedAgentApiToken; onDone: () => void }) {
  return (
    <div
      role="status"
      className="space-y-2 rounded-lg border border-[var(--status-succeeded)]/40 bg-[var(--status-succeeded)]/10 p-3"
    >
      <p className="text-xs font-medium">
        Token {issued.token.name ? `“${issued.token.name}” ` : ''}issued. Copy it now: it is not
        shown again.
      </p>
      <div className="flex items-center gap-1">
        <code
          className="min-w-0 flex-1 truncate rounded bg-background px-2 py-1 font-mono text-xs"
          aria-label="Token"
        >
          {issued.secret}
        </code>
        <CopyButton value={issued.secret} label="Copy token" />
      </div>
      <Button size="xs" variant="outline" onClick={onDone}>
        Done
      </Button>
    </div>
  );
}

function TokenRow({
  token,
  agentId,
  onRevoke,
}: {
  token: AgentApiToken;
  agentId: string;
  onRevoke: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const queryClient = useQueryClient();
  const revoke = useMutation({
    mutationFn: () => revokeAgentToken(agentId, token.id),
    onSuccess: () => {
      toast.success(`Token ${token.prefix}… revoked`);
      void queryClient.invalidateQueries({ queryKey: qk.agents.tokens(agentId) });
      onRevoke();
    },
    onError: (err) => toast.error(errorMessage(err)),
  });
  const revoked = token.revoked_at !== null;
  return (
    <li className="flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-2 text-xs">
      <KeyRoundIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
      <span className="min-w-0 flex-1">
        <span className={revoked ? 'text-muted-foreground line-through' : 'font-medium'}>
          {token.name || 'unnamed'}
        </span>
        <span className="text-muted-foreground">
          {' '}
          · <span className="font-mono">{token.prefix}…</span> · issued{' '}
          <time dateTime={token.created_at}>{formatRelative(token.created_at)}</time> · last used{' '}
          {token.last_used_at ? (
            <time dateTime={token.last_used_at}>{formatRelative(token.last_used_at)}</time>
          ) : (
            'never'
          )}
        </span>
      </span>
      {revoked ? (
        <Badge variant="outline" className="h-5 text-[10px]">
          revoked {formatDateTime(token.revoked_at ?? '')}
        </Badge>
      ) : (
        <>
          <Button
            size="xs"
            variant="outline"
            disabled={revoke.isPending}
            onClick={() => setConfirming(true)}
          >
            <XIcon />
            Revoke
          </Button>
          <ConfirmDialog
            open={confirming}
            onOpenChange={setConfirming}
            title={`Revoke token ${token.prefix}…?`}
            description="Calls the agent makes with it are refused at once. This cannot be undone."
            confirmLabel="Revoke"
            destructive
            onConfirm={() => revoke.mutate()}
          />
        </>
      )}
    </li>
  );
}

interface AgentTokensProps {
  agentId: string;
  /** Whether the viewer may issue and revoke (workspace admins); others see a note. */
  admin: boolean;
}

/**
 * The API tokens of an external agent (CONTRACT §8): the ones issued, with their prefix, when
 * they were issued, last used and revoked; a form to issue one, whose value is shown once; a
 * revoke on each. Admins only: the list itself is theirs to read.
 */
export function AgentTokens({ agentId, admin }: AgentTokensProps) {
  const queryClient = useQueryClient();
  const { data, isPending, error } = useQuery({ ...agentTokensQuery(agentId), enabled: admin });
  const [name, setName] = useState('');
  const [issued, setIssued] = useState<IssuedAgentApiToken | null>(null);
  const issue = useMutation({
    mutationFn: () => issueAgentToken(agentId, name.trim()),
    onSuccess: (next) => {
      setIssued(next);
      setName('');
      void queryClient.invalidateQueries({ queryKey: qk.agents.tokens(agentId) });
    },
    onError: (err) => toast.error(errorMessage(err)),
  });

  return (
    <section className="space-y-2">
      <h3 className="text-xs font-medium text-muted-foreground">API tokens</h3>
      <p className="text-xs text-muted-foreground">
        An external agent presents one of these on the Agent Firewall API; every call is decided
        under this agent's policy and recorded on its trace.
      </p>
      {!admin ? (
        <p className="text-xs text-muted-foreground">Workspace admins issue and revoke tokens.</p>
      ) : isPending ? (
        <Skeleton className="h-12 rounded-lg" />
      ) : error ? (
        <p role="alert" className="text-xs text-destructive">
          {errorMessage(error)}
        </p>
      ) : (
        <>
          {data.length === 0 ? (
            <p className="text-xs text-muted-foreground">No token issued yet.</p>
          ) : (
            <ul className="divide-y rounded-lg border" aria-label="API tokens">
              {data.map((token) => (
                <TokenRow
                  key={token.id}
                  token={token}
                  agentId={agentId}
                  onRevoke={() => {
                    if (issued?.token.id === token.id) setIssued(null);
                  }}
                />
              ))}
            </ul>
          )}
          {issued ? (
            <IssuedSecret issued={issued} onDone={() => setIssued(null)} />
          ) : (
            <form
              className="flex items-end gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                issue.mutate();
              }}
            >
              <div className="min-w-0 flex-1 space-y-1">
                <Label htmlFor={`token-name-${agentId}`} className="text-xs">
                  Name (optional)
                </Label>
                <Input
                  id={`token-name-${agentId}`}
                  value={name}
                  maxLength={TOKEN_NAME_MAX}
                  placeholder="e.g. the sidecar in production"
                  onChange={(event) => setName(event.target.value)}
                />
              </div>
              <AnimatedButton type="submit" size="sm" loading={issue.isPending}>
                <KeyRoundIcon />
                Issue token
              </AnimatedButton>
            </form>
          )}
        </>
      )}
    </section>
  );
}
