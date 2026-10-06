import { useMutation, useQueryClient, useSuspenseQuery } from '@tanstack/react-query';
import { FolderKanbanIcon, PlusIcon, Trash2Icon } from 'lucide-react';
import { Suspense, useState } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import { createProject, deleteProject, projectsQuery, updateProject } from '@/features/issues/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';
import type { Project, ProjectStatus } from '@/schemas/issue';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

const STATUS_OPTIONS: { value: ProjectStatus; label: string }[] = [
  { value: 'planned', label: 'Planned' },
  { value: 'started', label: 'In progress' },
  { value: 'paused', label: 'Paused' },
  { value: 'completed', label: 'Completed' },
  { value: 'canceled', label: 'Canceled' },
];

function ProjectCard({ workspace, project }: { workspace: Workspace; project: Project }) {
  const queryClient = useQueryClient();
  const [deleting, setDeleting] = useState(false);
  const refresh = () => queryClient.invalidateQueries({ queryKey: qk.issues.all });
  const canEdit = workspace.role !== 'guest';
  const setStatus = useMutation({
    mutationFn: (status: ProjectStatus) => updateProject(workspace.id, project.id, { status }),
    onSuccess: refresh,
  });
  const remove = useMutation({
    mutationFn: () => deleteProject(workspace.id, project.id),
    meta: { successMessage: 'Project deleted' },
    onSuccess: refresh,
  });
  const done =
    project.issue_count > 0 ? Math.round((project.closed_count / project.issue_count) * 100) : 0;
  return (
    <li className="space-y-3 rounded-xl border p-4">
      <div className="flex items-center gap-2">
        <h2 className="min-w-0 flex-1 truncate font-medium">{project.name}</h2>
        {canEdit ? (
          <OptionSelect
            value={project.status}
            onValueChange={(status) => setStatus.mutate(status)}
            options={STATUS_OPTIONS}
            aria-label={`Status of ${project.name}`}
            className="w-36"
          />
        ) : (
          <span className="text-sm text-muted-foreground">
            {STATUS_OPTIONS.find((o) => o.value === project.status)?.label}
          </span>
        )}
        {isWorkspaceAdmin(workspace.role) && (
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-destructive"
            aria-label={`Delete ${project.name}`}
            onClick={() => setDeleting(true)}
          >
            <Trash2Icon />
          </Button>
        )}
      </div>
      {project.description && (
        <p className="text-sm text-muted-foreground">{project.description}</p>
      )}
      <div className="space-y-1">
        <Progress value={done} aria-label={`${done}% of issues closed`} className="h-1.5" />
        <p className="text-xs text-muted-foreground">
          {project.issue_count === 0
            ? 'No issues yet'
            : `${project.closed_count} of ${project.issue_count} issues closed`}
          {project.target_date ? ` · target ${project.target_date}` : ''}
        </p>
      </div>
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={`Delete ${project.name}?`}
        description="The project is removed. Its issues stay, without a project."
        confirmLabel="Delete project"
        destructive
        onConfirm={() => remove.mutate()}
      />
    </li>
  );
}

function ProjectList({ workspace }: { workspace: Workspace }) {
  const { data: projects } = useSuspenseQuery(projectsQuery(workspace.id));
  if (projects.length === 0) {
    return (
      <EmptyState
        icon={FolderKanbanIcon}
        title="No projects yet"
        description="A project groups issues from any team around one outcome, such as a launch."
      />
    );
  }
  return (
    <ul className="grid gap-3 md:grid-cols-2">
      {projects.map((project) => (
        <ProjectCard key={project.id} workspace={workspace} project={project} />
      ))}
    </ul>
  );
}

function Projects({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const create = useMutation({
    mutationFn: () => createProject(workspace.id, { name: name.trim() }),
    meta: { successMessage: 'Project created' },
    onSuccess: () => {
      setName('');
      void queryClient.invalidateQueries({ queryKey: qk.issues.projects(workspace.id) });
    },
  });
  return (
    <div className="mx-auto w-full max-w-5xl space-y-6 p-4 sm:p-6">
      <PageHeader
        title="Projects"
        description={`Outcomes ${workspace.name} is working towards, with the issues that get them there.`}
      />
      {workspace.role !== 'guest' && (
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) create.mutate();
          }}
        >
          <Input
            value={name}
            maxLength={200}
            placeholder="New project name"
            aria-label="New project name"
            onChange={(e) => setName(e.target.value)}
            className="max-w-sm"
          />
          <Button type="submit" disabled={!name.trim() || create.isPending}>
            <PlusIcon />
            Add project
          </Button>
        </form>
      )}
      <Suspense fallback={<PageSkeleton />}>
        <ProjectList workspace={workspace} />
      </Suspense>
    </div>
  );
}

export default function ProjectsPage() {
  const { current } = useCurrentWorkspace();
  return (
    <>
      <Seo title="Projects" noIndex />
      {current ? <Projects key={current.id} workspace={current} /> : <PageSkeleton />}
    </>
  );
}
