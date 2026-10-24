import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowLeftIcon, FolderKanbanIcon, Trash2Icon } from 'lucide-react';
import { useState, type ReactNode } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { IconButton } from '@/components/custom-ui/icon-button';
import { ProjectStatusGlyph, TeamChip } from '@/components/custom-ui/issue-glyphs';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import { Textarea } from '@/components/ui/textarea';
import { deleteProject, projectsQuery, updateProject } from '@/features/issues/api';
import {
  ProjectLeadSelect,
  ProjectStatusSelect,
  ProjectTeamSelect,
} from '@/features/issues/project-fields';
import { projectProgress, projectStatusLabel } from '@/features/issues/project-status';
import { membersQuery, teamsQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { Project } from '@/schemas/issue';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

import { IssueExplorer } from './issues-page';

/** One property of the project in the row under its name: a small label before its control. */
function Property({
  label,
  className,
  children,
}: {
  label: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={cn('flex items-center gap-2', className)}>
      <span className="text-xs text-muted-foreground">{label}</span>
      {children}
    </div>
  );
}

/** The project's name, description and properties; everything saves as it is changed. */
function ProjectHeader({ workspace, project }: { workspace: Workspace; project: Project }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [name, setName] = useState(project.name);
  const [description, setDescription] = useState(project.description);
  const [deleting, setDeleting] = useState(false);
  const canEdit = workspace.role !== 'guest';
  const { data: members = [] } = useQuery({ ...membersQuery(workspace.id), enabled: canEdit });
  const { data: teams = [] } = useQuery({ ...teamsQuery(workspace.id), enabled: canEdit });
  // The owning team is always one the caller may see; it still names itself while the list loads.
  const teamOptions =
    project.team && !teams.some((t) => t.id === project.team?.id)
      ? [...teams, project.team]
      : teams;
  const refresh = () => queryClient.invalidateQueries({ queryKey: qk.issues.all });
  const save = useMutation({
    mutationFn: (body: Parameters<typeof updateProject>[2]) =>
      updateProject(workspace.id, project.id, body),
    onSuccess: refresh,
  });
  const remove = useMutation({
    mutationFn: () => deleteProject(workspace.id, project.id),
    meta: { successMessage: 'Project deleted' },
    onSuccess: () => {
      void refresh();
      void navigate('/app/projects');
    },
  });
  const commitText = () => {
    const next = name.trim();
    if (!next) {
      setName(project.name);
      return;
    }
    if (next !== project.name || description !== project.description) {
      save.mutate({ name: next, description });
    }
  };
  const done = projectProgress(project);

  return (
    <header className="shrink-0 space-y-3 border-b pb-4">
      {/* The name is edited in place; the heading names the page for assistive technology. */}
      <h1 className="sr-only">{project.name}</h1>
      <div className="flex items-start gap-2">
        <FolderKanbanIcon className="mt-1.5 size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1 space-y-1">
          <input
            value={name}
            maxLength={200}
            readOnly={!canEdit}
            aria-label="Project name"
            className="-mx-1 w-full rounded-md bg-transparent px-1 text-lg font-semibold tracking-tight outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
            onChange={(e) => setName(e.target.value)}
            onBlur={commitText}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur();
            }}
          />
          <Textarea
            value={description}
            rows={1}
            readOnly={!canEdit}
            aria-label="Project description"
            placeholder={canEdit ? 'Add a short summary of the outcome…' : ''}
            className="-mx-1 min-h-0 resize-none rounded-md border-0 bg-transparent px-1 py-0 text-[13px] text-muted-foreground shadow-none focus-visible:ring-2 focus-visible:ring-ring/50 dark:bg-transparent"
            onChange={(e) => setDescription(e.target.value)}
            onBlur={commitText}
          />
        </div>
        {isWorkspaceAdmin(workspace.role) && (
          <IconButton
            className="text-muted-foreground hover:text-destructive"
            label={`Delete ${project.name}`}
            onClick={() => setDeleting(true)}
          >
            <Trash2Icon />
          </IconButton>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-[13px]">
        <Property label="Status">
          {canEdit ? (
            <ProjectStatusSelect
              value={project.status}
              onValueChange={(status) => save.mutate({ status })}
              className="w-36"
            />
          ) : (
            <span className="flex items-center gap-1.5">
              <ProjectStatusGlyph status={project.status} />
              {projectStatusLabel(project.status)}
            </span>
          )}
        </Property>
        {canEdit && (
          <Property label="Lead">
            <ProjectLeadSelect
              members={members}
              value={project.lead_id}
              onValueChange={(lead_id) => save.mutate({ lead_id })}
              className="w-44"
            />
          </Property>
        )}
        <Property label="Team">
          {canEdit ? (
            <ProjectTeamSelect
              teams={teamOptions}
              value={project.team_id}
              onValueChange={(team_id) => save.mutate({ team_id })}
              className="w-44"
            />
          ) : project.team ? (
            <TeamChip team={project.team} />
          ) : (
            <span className="text-muted-foreground">No team</span>
          )}
        </Property>
        <Property label="Target">
          {canEdit ? (
            <Input
              type="date"
              value={project.target_date ?? ''}
              aria-label="Target date"
              className="h-8 w-40 text-[13px]"
              onChange={(e) => save.mutate({ target_date: e.target.value || null })}
            />
          ) : (
            <span className="tabular-nums">{project.target_date ?? '—'}</span>
          )}
        </Property>
        <Property label="Progress" className="sm:ml-auto">
          <span className="flex w-36 items-center gap-2">
            <Progress
              value={done}
              aria-label={`${done}% of issues closed`}
              className="h-1.5 flex-1"
            />
            <span className="text-xs text-muted-foreground tabular-nums">
              {project.closed_count}/{project.issue_count}
            </span>
          </span>
        </Property>
      </div>
      {save.error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(save.error)}
        </p>
      )}
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={`Delete ${project.name}?`}
        description="The project is removed. Its issues stay, without a project."
        confirmLabel="Delete project"
        destructive
        onConfirm={() => remove.mutate()}
      />
    </header>
  );
}

function ProjectView({ workspace, projectId }: { workspace: Workspace; projectId: string }) {
  const { data: projects, isPending, error, refetch } = useQuery(projectsQuery(workspace.id));
  if (isPending) return <PageSkeleton />;
  // Only a project list that was never read stands in for the page: once read, it stays on
  // screen when a later refetch fails.
  if (!projects) {
    return (
      <PageContainer>
        <h1 className="sr-only">Project</h1>
        <ErrorState
          title="Couldn’t load the project"
          error={error}
          onRetry={() => void refetch()}
        />
      </PageContainer>
    );
  }
  const project = projects.find((p) => p.id === projectId);
  if (!project) {
    return (
      <PageContainer>
        <EmptyState
          icon={FolderKanbanIcon}
          title="No such project"
          headingLevel={1}
          description="It may have been deleted, or it belongs to another workspace."
          action={
            <Button variant="outline" asChild>
              <Link to="/app/projects">
                <ArrowLeftIcon />
                All projects
              </Link>
            </Button>
          }
        />
      </PageContainer>
    );
  }
  return (
    // From md up the header stays put and the issues fill the rest of the height: a board reaches
    // the bottom edge with columns that scroll on their own, and a list scrolls under the header.
    <PageContainer className="flex flex-col md:min-h-0 md:flex-1">
      <Seo title={project.name} noIndex />
      <ProjectHeader key={project.id} workspace={workspace} project={project} />
      {/* The padding, taken back by the margin, leaves room for focus rings at the edges. */}
      <div className="flex flex-col md:-m-1 md:min-h-0 md:flex-1 md:overflow-y-auto md:p-1">
        <IssueExplorer
          workspace={workspace}
          project={project}
          defaultTeamId={project.team_id ?? undefined}
        />
      </div>
    </PageContainer>
  );
}

export default function ProjectPage() {
  const { current } = useCurrentWorkspace();
  const { projectId = '' } = useParams();
  return current ? (
    <ProjectView key={`${current.id}:${projectId}`} workspace={current} projectId={projectId} />
  ) : (
    <PageSkeleton />
  );
}
