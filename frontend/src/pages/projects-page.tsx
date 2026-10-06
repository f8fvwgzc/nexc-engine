import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from '@tanstack/react-query';
import { FolderKanbanIcon, PlusIcon } from 'lucide-react';
import { Suspense, useState } from 'react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import { createProject, projectsQuery } from '@/features/issues/api';
import { projectProgress, projectStatusLabel } from '@/features/issues/project-status';
import { membersQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';
import type { Workspace } from '@/schemas/workspace';

function ProjectList({ workspace }: { workspace: Workspace }) {
  const { data: projects } = useSuspenseQuery(projectsQuery(workspace.id));
  // Guests cannot list members; a lead then shows without a name.
  const { data: members = [] } = useQuery({
    ...membersQuery(workspace.id),
    enabled: workspace.role !== 'guest',
  });
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
    <div className="overflow-hidden rounded-lg border">
      <div className="hidden h-8 items-center gap-3 border-b bg-muted/40 px-3 text-xs text-muted-foreground sm:flex">
        <span className="flex-1">Name</span>
        <span className="w-24">Status</span>
        <span className="w-8">Lead</span>
        <span className="w-24">Target</span>
        <span className="w-40">Progress</span>
      </div>
      <ul className="divide-y">
        {projects.map((project) => {
          const done = projectProgress(project);
          return (
            <li key={project.id}>
              <Link
                to={`/app/projects/${project.id}`}
                className="flex min-h-10 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-[13px] transition-colors outline-none hover:bg-muted/60 focus-visible:bg-muted/60"
              >
                <span className="flex min-w-0 flex-1 basis-48 items-center gap-2">
                  <FolderKanbanIcon className="size-3.5 shrink-0 text-muted-foreground" />
                  <span className="truncate font-medium">{project.name}</span>
                </span>
                <span className="w-24 text-muted-foreground">
                  {projectStatusLabel(project.status)}
                </span>
                <span className="w-8">
                  <PersonGlyph name={members.find((m) => m.user_id === project.lead_id)?.name} />
                </span>
                <span className="w-24 text-muted-foreground tabular-nums">
                  {project.target_date ?? '—'}
                </span>
                <span className="flex w-40 items-center gap-2">
                  <Progress
                    value={done}
                    aria-label={`${done}% of issues closed`}
                    className="h-1.5 flex-1"
                  />
                  <span className="w-12 text-right text-xs text-muted-foreground tabular-nums">
                    {project.closed_count}/{project.issue_count}
                  </span>
                </span>
              </Link>
            </li>
          );
        })}
      </ul>
    </div>
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
    <div className="mx-auto w-full max-w-5xl space-y-5 p-4 sm:p-6">
      <PageHeader
        title="Projects"
        description={`Outcomes ${workspace.name} is working towards. Open one to see its issues.`}
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
            className="h-8 max-w-sm text-[13px]"
          />
          <Button type="submit" size="sm" disabled={!name.trim() || create.isPending}>
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
