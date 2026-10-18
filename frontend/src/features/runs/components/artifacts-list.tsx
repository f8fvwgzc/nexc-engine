import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  DownloadIcon,
  FileArchiveIcon,
  FileIcon,
  LockIcon,
  LockOpenIcon,
  PackageOpenIcon,
} from 'lucide-react';
import { toast } from 'sonner';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { releaseArtifact } from '@/features/policies/api';
import { errorMessage } from '@/lib/api/errors';
import { formatBytes, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { isDownloadable, type Artifact } from '@/schemas/run';

import { artifactsQuery, downloadArtifact, downloadArtifactsZip } from '../api';

function download(task: Promise<void>) {
  task.catch((error: unknown) => toast.error(errorMessage(error)));
}

interface ArtifactsListProps {
  runId: string;
  titles: Map<string, string>;
  /** Whether the viewer may release quarantined files (workspace admins). */
  admin?: boolean;
}

export function ArtifactsList({ runId, titles, admin = false }: ArtifactsListProps) {
  const queryClient = useQueryClient();
  const { data: artifacts, isPending } = useQuery(artifactsQuery(runId));
  const release = useMutation({
    mutationFn: releaseArtifact,
    onSuccess: (released) => {
      toast.success(`${released.path} can be downloaded again`);
      queryClient.setQueryData<Artifact[]>(qk.runs.artifacts(runId), (list) =>
        list?.map((a) => (a.id === released.id ? released : a)),
      );
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

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
  const downloadable = artifacts.filter(isDownloadable);

  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <Button
          variant="outline"
          size="sm"
          disabled={downloadable.length === 0}
          onClick={() => download(downloadArtifactsZip(runId))}
        >
          <FileArchiveIcon />
          Download all (.zip)
        </Button>
      </div>
      <ul className="divide-y rounded-xl border">
        {artifacts.map((artifact: Artifact) => {
          const kept = !isDownloadable(artifact);
          return (
            <li key={artifact.id} className="flex items-center gap-3 px-4 py-3">
              {kept ? (
                <Tooltip>
                  <TooltipTrigger className="shrink-0 text-destructive" aria-label="Quarantined">
                    <LockIcon className="size-4" />
                  </TooltipTrigger>
                  <TooltipContent className="max-w-sm whitespace-pre-wrap">
                    Kept in by the DLP screen: {artifact.quarantine_reason}. An admin can release
                    it.
                  </TooltipContent>
                </Tooltip>
              ) : (
                <FileIcon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
              )}
              <div className="min-w-0 flex-1">
                <p className={`truncate font-mono text-sm ${kept ? 'text-muted-foreground' : ''}`}>
                  {artifact.path}
                </p>
                <p className="truncate text-xs text-muted-foreground">
                  {titles.get(artifact.node_id) ?? 'Node'} · {artifact.mime} ·{' '}
                  {formatBytes(artifact.size)} · {formatRelative(artifact.created_at)}
                  {kept && (
                    <span className="text-destructive">
                      {' '}
                      · quarantined: {artifact.quarantine_reason}
                    </span>
                  )}
                  {artifact.released_at && (
                    <span> · released {formatRelative(artifact.released_at)}</span>
                  )}
                </p>
              </div>
              {kept && admin && (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={release.isPending}
                  onClick={() => release.mutate(artifact.id)}
                >
                  <LockOpenIcon />
                  Release
                </Button>
              )}
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Download ${artifact.path}`}
                disabled={kept}
                onClick={() => download(downloadArtifact(artifact))}
              >
                <DownloadIcon />
              </Button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
