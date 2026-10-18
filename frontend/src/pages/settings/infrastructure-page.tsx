import { useMutation, useQuery } from '@tanstack/react-query';
import { ServerIcon } from 'lucide-react';
import { useState } from 'react';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { checkConnection, infrastructureQuery } from '@/features/insight/api';
import { errorMessage } from '@/lib/api/errors';
import { formatBytes } from '@/lib/format';
import { useAuthStore } from '@/stores/auth-store';

function Row({ name, children }: { name: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5 px-3 py-2 text-[13px]">
      <dt className="w-40 shrink-0 text-muted-foreground">{name}</dt>
      <dd className="min-w-0 flex-1 break-words">{children}</dd>
    </div>
  );
}

function Status() {
  const { data, isPending, error } = useQuery(infrastructureQuery());
  if (isPending) return <Skeleton className="h-64 w-full rounded-lg" />;
  if (error) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  const db = data.database;
  const behind = db.migrations_known - db.migrations_applied;
  return (
    <dl className="divide-y rounded-lg border">
      <Row name="Database">
        <span className="font-mono text-xs">{db.location}</span>
      </Row>
      <Row name="Version">
        {db.version} · {formatBytes(db.size_bytes)}
      </Row>
      <Row name="Vector search">
        {db.pgvector ? `pgvector ${db.pgvector}` : 'Off: the database has no pgvector extension'}
      </Row>
      <Row name="Schema">
        {db.migrations_applied} of {db.migrations_known} migrations applied
        {behind > 0 && <Badge variant="destructive">restart to apply {behind}</Badge>}
      </Row>
      <Row name="Agent runtime">
        <span className="font-mono text-xs">{data.runtime_url}</span>{' '}
        <Badge variant={data.runtime_reachable ? 'outline' : 'destructive'}>
          {data.runtime_reachable ? 'answers' : 'not reachable'}
        </Badge>
      </Row>
      <Row name="Runtime stream">
        {data.runtime_grpc.enabled ? (
          <>
            <span className="font-mono text-xs">{data.runtime_grpc.url}</span>{' '}
            <Badge variant={data.runtime_grpc.ok ? 'outline' : 'destructive'}>
              {data.runtime_grpc.ok ? 'gRPC answers' : 'gRPC not reachable'}
            </Badge>
            {data.runtime_grpc.detail && (
              <span className="text-muted-foreground"> · {data.runtime_grpc.detail}</span>
            )}
          </>
        ) : (
          `NDJSON over HTTP (NEXC_RUNTIME_PROTOCOL=${data.runtime_protocol}); the gRPC stream is the default and NDJSON is kept for one release`
        )}
      </Row>
      <Row name="Tools gateway">
        {data.gateway.calls_per_minute} calls per minute per agent · {data.gateway.timeout_secs} s
        per call · responses up to {formatBytes(data.gateway.max_response_bytes)}
      </Row>
      <Row name="Agent Firewall API">
        {data.firewall.enabled ? (
          <>
            <span className="font-mono text-xs">{data.firewall.addr}</span>{' '}
            <Badge variant="outline">
              {data.firewall.open_sessions} open session
              {data.firewall.open_sessions === 1 ? '' : 's'}
            </Badge>
            <span className="text-muted-foreground">
              {' '}
              · signs with Ed25519 key{' '}
              <span className="font-mono text-xs">{data.firewall.public_key.slice(0, 16)}…</span>
            </span>
          </>
        ) : (
          'Off: set NEXC_GRPC_ADDR to serve external agents over gRPC'
        )}
      </Row>
      <Row name="OpenTelemetry">
        {data.otlp.enabled ? (
          <>
            <span className="font-mono text-xs">{data.otlp.endpoint}</span>{' '}
            <Badge variant={data.otlp.reachable ? 'outline' : 'destructive'}>
              {data.otlp.reachable ? 'collector answers' : 'collector not reachable'}
            </Badge>
            <span className="text-muted-foreground">
              {' '}
              · {data.otlp.spans_exported} spans exported
              {data.otlp.behind > 0 && `, ${data.otlp.behind} behind`}
              {data.otlp.last_error && ` · ${data.otlp.last_error}`}
            </span>
          </>
        ) : (
          'Off: set NEXC_OTLP_ENDPOINT to export the trace as OpenTelemetry spans'
        )}
      </Row>
      <Row name="Embedding model">{data.embedding_model}</Row>
      <Row name="Audit graph">
        {data.ontology.enabled ? (
          <>
            <span className="font-mono text-xs">{data.ontology.url}</span>{' '}
            <Badge variant={data.ontology.ok ? 'outline' : 'destructive'}>
              {data.ontology.ok ? 'answers' : 'not reachable'}
            </Badge>
            {data.ontology.detail && (
              <span className="text-muted-foreground"> · {data.ontology.detail}</span>
            )}
          </>
        ) : (
          'Off: set NEXC_ONTO_URL to an onto-intelli server to ask the audit'
        )}
      </Row>
      <Row name="Background queue">{data.queue}</Row>
      <Row name="Cache">{data.cache}</Row>
    </dl>
  );
}

/** Asks the server whether it can reach another PostgreSQL or Redis. Nothing is changed or kept. */
function Check() {
  const [url, setUrl] = useState('');
  const check = useMutation({
    mutationFn: () => checkConnection(url.trim()),
    meta: { errorToast: false },
  });
  const found = check.data;
  return (
    <section aria-label="Check a connection" className="space-y-3 rounded-lg border p-4">
      <div className="space-y-1">
        <h2 className="text-[13px] font-medium">Check a connection</h2>
        <p className="text-[13px] text-muted-foreground">
          Before moving the server to another database, check that it can reach it. The address is
          used once and not kept. To switch, copy the data over (pg_dump, pg_restore), set
          NEXC_DATABASE_URL and restart; an empty database is set up when the server starts on it.
        </p>
      </div>
      <form
        className="flex flex-wrap gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (url.trim()) check.mutate();
        }}
      >
        <Input
          type="password"
          value={url}
          autoComplete="off"
          placeholder="postgres://user:password@host:5432/db  or  redis://host:6379"
          aria-label="Connection URL"
          className="h-8 min-w-64 flex-1 font-mono text-xs"
          onChange={(e) => setUrl(e.target.value)}
        />
        <Button type="submit" size="sm" disabled={!url.trim() || check.isPending}>
          {check.isPending ? 'Checking…' : 'Check'}
        </Button>
      </form>
      {check.error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(check.error)}
        </p>
      )}
      {found && (
        <p role="status" className="text-[13px]">
          <Badge variant={found.reachable ? 'outline' : 'destructive'}>
            {found.reachable ? 'Reachable' : 'Not reachable'}
          </Badge>{' '}
          {found.detail}
          {found.pgvector_available !== null &&
            ` · pgvector ${found.pgvector_available ? 'available' : 'not available'}`}
          {found.migrations_applied !== null &&
            ` · ${found.migrations_applied === 0 ? 'empty (no nexc tables yet)' : `${found.migrations_applied} migrations applied`}`}
        </p>
      )}
    </section>
  );
}

export default function InfrastructurePage() {
  const admin = useAuthStore((s) => s.user?.role === 'admin');
  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <Seo title="Infrastructure" noIndex />
      <PageHeader
        title="Infrastructure"
        description="What this server runs on. Its connections come from its environment and are read when it starts."
      />
      {admin ? (
        <>
          <Status />
          <Check />
        </>
      ) : (
        <EmptyState
          icon={ServerIcon}
          title="Only administrators of this server see its infrastructure"
          description="This is about the installation, not about one workspace."
        />
      )}
    </div>
  );
}
