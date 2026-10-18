import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowLeftIcon, FolderKanbanIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { IconButton } from '@/components/custom-ui/icon-button';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import { Textarea } from '@/components/ui/textarea';
import { deleteProject, projectsQuery, updateProject } from '@/features/issues/api';
import {
  PROJECT_STATUS_OPTIONS,
  projectProgress,
  projectStatusLabel,
} from '@/features/issues/project-status';
import { membersQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { Project } from '@/schemas/issue';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

import { IssueExplorer } from './issues-page';

const NONE = '__none__';

/** The project's name, description and properties; everything saves as it is changed. */
function ProjectHeader({ workspace, project }: { workspace: Workspace; project: Project }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [name, setName] = useState(project.name);
  const [description, setDescription] = useState(project.description);
  const [deleting, setDeleting] = useState(false);
  const canEdit = workspace.role !== 'guest';
  const { data: members = [] } = useQuery({ ...membersQuery(workspace.id), enabled: canEdit });
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
    <header className="space-y-3 border-b pb-4">
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
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[13px]">
        {canEdit ? (
          <>
            <OptionSelect
              value={project.status}
              onValueChange={(status) => save.mutate({ status })}
              options={PROJECT_STATUS_OPTIONS}
              aria-label="Status"
              className="w-36"
            />
            <OptionSelect
              value={project.lead_id ?? NONE}
              onValueChange={(id) => save.mutate({ lead_id: id === NONE ? null : id })}
              options={[
                { value: NONE, label: 'No lead' },
                ...members.map((m) => ({ value: m.user_id, label: m.name })),
              ]}
              aria-label="Lead"
              className="w-44"
            />
            <label className="flex items-center gap-2 text-muted-foreground">
              Target
              <Input
                type="date"
                value={project.target_date ?? ''}
                className="h-8 w-40 text-[13px] text-foreground"
                onChange={(e) => save.mutate({ target_date: e.target.value || null })}
              />
            </label>
          </>
        ) : (
          <span className="text-muted-foreground">
            {projectStatusLabel(project.status)}
            {project.target_date ? ` · target ${project.target_date}` : ''}
          </span>
        )}
        <span className="flex w-full items-center gap-2 sm:ml-auto sm:w-56">
          <Progress
            value={done}
            aria-label={`${done}% of issues closed`}
            className="h-1.5 flex-1"
          />
          <span className="text-xs text-muted-foreground tabular-nums">
            {project.closed_count}/{project.issue_count} closed
          </span>
        </span>
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
    <PageContainer>
      <Seo title={project.name} noIndex />
      <ProjectHeader key={project.id} workspace={workspace} project={project} />
      <IssueExplorer workspace={workspace} project={project} />
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
