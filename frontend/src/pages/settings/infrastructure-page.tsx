import { useMutation, useQuery } from '@tanstack/react-query';
import { ServerIcon } from 'lucide-react';
import { useState } from 'react';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { ScrollRegion } from '@/components/custom-ui/scroll-region';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { checkConnection, infrastructureQuery, latencyQuery } from '@/features/insight/api';
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

function ms(value: number): string {
  return value >= 100 ? value.toFixed(0) : value >= 10 ? value.toFixed(1) : value.toFixed(2);
}

/** Most rows shown; the rest are summed up in a line. */
const LATENCY_ROWS = 40;

/** p50, p99 and max per endpoint over the last five minutes, from the instance that answers. */
function Latency() {
  const { data, isPending, error } = useQuery(latencyQuery());
  if (isPending) return <Skeleton className="h-40 w-full rounded-lg" />;
  if (error) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  const shown = data.rows.slice(0, LATENCY_ROWS);
  const hidden = data.rows.length - shown.length;
  const minutes = Math.round(data.window_secs / 60);
  return (
    <section aria-label="Latency" className="space-y-3 rounded-lg border p-4">
      <div className="space-y-1">
        <h2 className="text-[13px] font-medium">Latency, last {minutes} minutes</h2>
        <p className="text-[13px] text-muted-foreground">
          Per endpoint, from this server instance's own histograms (behind a load balancer, one
          replica's). gRPC rows are the calls the server made to the runtime, onto-intelli and the
          collector. The same figures over time: the Grafana dashboard in deploy/grafana.
        </p>
      </div>
      {shown.length === 0 ? (
        <p className="text-[13px] text-muted-foreground">Nothing answered in the window yet.</p>
      ) : (
        <ScrollRegion label="Latency per endpoint" className="overflow-x-auto">
          <table className="w-full text-[13px]">
            <thead className="text-left text-muted-foreground">
              <tr>
                <th className="py-1 pr-3 font-medium">Endpoint</th>
                <th className="py-1 pr-3 text-right font-medium">Calls</th>
                <th className="py-1 pr-3 text-right font-medium">p50 ms</th>
                <th className="py-1 pr-3 text-right font-medium">p99 ms</th>
                <th className="py-1 text-right font-medium">max ms</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((row) => (
                <tr key={`${row.method} ${row.route}`} className="border-t">
                  <td className="py-1 pr-3 font-mono text-xs">
                    <span className="text-muted-foreground">{row.method}</span> {row.route}
                  </td>
                  <td className="py-1 pr-3 text-right tabular-nums">{row.count}</td>
                  <td className="py-1 pr-3 text-right tabular-nums">{ms(row.p50_ms)}</td>
                  <td className="py-1 pr-3 text-right tabular-nums">{ms(row.p99_ms)}</td>
                  <td className="py-1 text-right tabular-nums">{ms(row.max_ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {hidden > 0 && (
            <p className="pt-2 text-[13px] text-muted-foreground">
              {hidden} more endpoint{hidden === 1 ? '' : 's'} with a lower p99.
            </p>
          )}
        </ScrollRegion>
      )}
    </section>
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
    <PageContainer width="narrow">
      <Seo title="Infrastructure" noIndex />
      <PageHeader
        title="Infrastructure"
        description="What this server runs on. Its connections come from its environment and are read when it starts."
      />
      {admin ? (
        <>
          <Status />
          <Latency />
          <Check />
        </>
      ) : (
        <EmptyState
          icon={ServerIcon}
          title="Only administrators of this server see its infrastructure"
          description="This is about the installation, not about one workspace."
        />
      )}
    </PageContainer>
  );
}
