import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { FolderKanbanIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { OptionSelect } from '@/components/custom-ui/option-select';
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
      <div className="flex items-start gap-2">
        <FolderKanbanIcon className="mt-1.5 size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1 space-y-1">
          <input
            value={name}
            maxLength={200}
            readOnly={!canEdit}
            aria-label="Project name"
            className="w-full bg-transparent text-lg font-semibold tracking-tight outline-none"
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
            className="min-h-0 resize-none border-0 bg-transparent p-0 text-[13px] text-muted-foreground shadow-none focus-visible:ring-0 dark:bg-transparent"
            onChange={(e) => setDescription(e.target.value)}
            onBlur={commitText}
          />
        </div>
        {isWorkspaceAdmin(workspace.role) && (
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-muted-foreground hover:text-destructive"
            aria-label={`Delete ${project.name}`}
            onClick={() => setDeleting(true)}
          >
            <Trash2Icon />
          </Button>
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
        <span className="ml-auto flex w-56 items-center gap-2">
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
  const { data: projects, isPending } = useQuery(projectsQuery(workspace.id));
  if (isPending) return <PageSkeleton />;
  const project = projects?.find((p) => p.id === projectId);
  if (!project) {
    return (
      <div className="p-6">
        <EmptyState
          icon={FolderKanbanIcon}
          title="No such project"
          description="It may have been deleted, or it belongs to another workspace."
        />
      </div>
    );
  }
  return (
    <div className="mx-auto w-full max-w-6xl space-y-5 p-4 sm:p-6">
      <Seo title={project.name} noIndex />
      <ProjectHeader key={project.id} workspace={workspace} project={project} />
      <IssueExplorer workspace={workspace} project={project} />
    </div>
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
