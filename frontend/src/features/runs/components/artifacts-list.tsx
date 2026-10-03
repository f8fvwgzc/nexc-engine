import { useQuery } from '@tanstack/react-query';
import { DownloadIcon, FileArchiveIcon, FileIcon, PackageOpenIcon } from 'lucide-react';
import { toast } from 'sonner';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { formatBytes, formatRelative } from '@/lib/format';
import type { Artifact } from '@/schemas/run';

import { artifactsQuery, downloadArtifact, downloadArtifactsZip } from '../api';

function download(task: Promise<void>) {
  task.catch((error: unknown) => toast.error(errorMessage(error)));
}

export function ArtifactsList({ runId, titles }: { runId: string; titles: Map<string, string> }) {
  const { data: artifacts, isPending } = useQuery(artifactsQuery(runId));

  if (isPending) return <Skeleton className="h-24 rounded-xl" />;
  if (!artifacts || artifacts.length === 0) {
    return (
      <EmptyState
        icon={PackageOpenIcon}
        title="No artifacts"
        description="Files produced by nodes (reports, code, data) appear here when the run creates them."
      />
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <Button variant="outline" size="sm" onClick={() => download(downloadArtifactsZip(runId))}>
          <FileArchiveIcon />
          Download all (.zip)
        </Button>
      </div>
      <ul className="divide-y rounded-xl border">
        {artifacts.map((artifact: Artifact) => (
          <li key={artifact.id} className="flex items-center gap-3 px-4 py-3">
            <FileIcon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
            <div className="min-w-0 flex-1">
              <p className="truncate font-mono text-sm">{artifact.path}</p>
              <p className="truncate text-xs text-muted-foreground">
                {titles.get(artifact.node_id) ?? 'Node'} · {artifact.mime} ·{' '}
                {formatBytes(artifact.size)} · {formatRelative(artifact.created_at)}
              </p>
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Download ${artifact.path}`}
              onClick={() => download(downloadArtifact(artifact))}
            >
              <DownloadIcon />
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}
