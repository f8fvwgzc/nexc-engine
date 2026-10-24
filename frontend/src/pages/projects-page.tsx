import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CalendarIcon, FolderKanbanIcon, KanbanIcon, ListIcon, PlusIcon } from 'lucide-react';
import { useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { IconButton } from '@/components/custom-ui/icon-button';
import { PersonGlyph, ProjectStatusGlyph, TeamChip } from '@/components/custom-ui/issue-glyphs';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { ScrollRegion } from '@/components/custom-ui/scroll-region';
import { PageSkeleton, TableSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { projectsQuery, updateProject } from '@/features/issues/api';
import { NewProjectDialog } from '@/features/issues/new-project-dialog';
import {
  PROJECT_LIST_ORDER,
  groupProjectsByStatus,
  projectProgress,
  projectStatusLabel,
} from '@/features/issues/project-status';
import { membersQuery, teamsQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';
import { cn } from '@/lib/utils';
import type { Project, ProjectStatus } from '@/schemas/issue';
import { teamLabel, type Workspace } from '@/schemas/workspace';

const VIEW_KEY = 'nexc.projects.view';
type View = 'list' | 'board';

function rememberedView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === 'board' ? 'board' : 'list';
  } catch {
    return 'list';
  }
}

const ALL = '__all__';

/** The name of a project's lead, when the members could be read. */
type LeadName = (userId: string | null) => string | undefined;

/** How many of a project's issues are closed, as a bar and a count. */
function ProjectProgress({ project, className }: { project: Project; className?: string }) {
  const done = projectProgress(project);
  return (
    <span className={cn('flex items-center gap-2', className)}>
      <Progress value={done} aria-label={`${done}% of issues closed`} className="h-1.5 flex-1" />
      <span className="text-xs text-muted-foreground tabular-nums">
        {project.closed_count}/{project.issue_count}
      </span>
    </span>
  );
}

function ProjectRow({ project, lead }: { project: Project; lead: string | undefined }) {
  return (
    <li>
      <Link
        to={`/app/projects/${project.id}`}
        className="flex min-h-10 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-[13px] transition-colors outline-none hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
      >
        <span className="flex min-w-0 flex-1 basis-48 items-center gap-2">
          <FolderKanbanIcon className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="truncate font-medium" title={project.name}>
            {project.name}
          </span>
        </span>
        <span className="flex w-40 min-w-0">
          {project.team ? (
            <TeamChip team={project.team} />
          ) : (
            <span className="text-muted-foreground">No team</span>
          )}
        </span>
        <span className="flex w-28 items-center gap-1.5 text-muted-foreground">
          <ProjectStatusGlyph status={project.status} />
          {projectStatusLabel(project.status)}
        </span>
        <span className="w-8">
          <PersonGlyph name={lead} />
        </span>
        <span className="w-24 text-muted-foreground tabular-nums">
          {project.target_date ?? '—'}
        </span>
        <ProjectProgress project={project} className="w-40" />
      </Link>
    </li>
  );
}

/** The projects grouped by status, the ones being worked on first. */
function ProjectList({ projects, leadName }: { projects: Project[]; leadName: LeadName }) {
  return (
    <div className="divide-y overflow-hidden rounded-lg border motion-safe:animate-fade-in">
      <div className="hidden h-8 items-center gap-3 bg-muted/40 px-3 text-xs text-muted-foreground sm:flex">
        <span className="flex-1">Name</span>
        <span className="w-40">Team</span>
        <span className="w-28">Status</span>
        <span className="w-8">Lead</span>
        <span className="w-24">Target</span>
        <span className="w-40">Progress</span>
      </div>
      {groupProjectsByStatus(projects, { order: PROJECT_LIST_ORDER }).map((group) => (
        <section key={group.status}>
          <h2 className="flex h-8 items-center gap-2 border-b bg-muted/20 px-3 text-[13px] font-medium">
            <ProjectStatusGlyph status={group.status} />
            {group.label}
            <span className="text-xs font-normal text-muted-foreground">
              {group.projects.length}
            </span>
          </h2>
          <ul className="divide-y">
            {group.projects.map((project) => (
              <ProjectRow key={project.id} project={project} lead={leadName(project.lead_id)} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

/**
 * One column per project status. Dragging a card to another column changes the project's status;
 * the project's own page offers the same change without a pointer. `onCreate` (members, not
 * guests) puts a "new project in this status" button on each column.
 */
function ProjectBoard({
  workspace,
  projects,
  leadName,
  onCreate,
}: {
  workspace: Workspace;
  projects: Project[];
  leadName: LeadName;
  onCreate?: (status: ProjectStatus) => void;
}) {
  const queryClient = useQueryClient();
  const [over, setOver] = useState<ProjectStatus | null>(null);
  const canMove = workspace.role !== 'guest';
  const lists = qk.issues.projectLists(workspace.id);
  const move = useMutation({
    mutationFn: ({ id, status }: { id: string; status: ProjectStatus }) =>
      updateProject(workspace.id, id, { status }),
    // The card lands in its new column at once; the lists are read again either way. A read
    // still in flight is dropped first, so it cannot put the card back in its old column.
    onMutate: async ({ id, status }) => {
      await queryClient.cancelQueries({ queryKey: lists });
      queryClient.setQueriesData<Project[]>({ queryKey: lists }, (list) =>
        list?.map((p) => (p.id === id ? { ...p, status } : p)),
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: lists }),
  });
  return (
    <ScrollRegion label="Project board" className="flex min-h-0 flex-1 gap-3 overflow-x-auto pb-2">
      {groupProjectsByStatus(projects, { keepEmpty: true }).map((column) => (
        <section
          key={column.status}
          aria-label={column.label}
          data-over={over === column.status}
          className="flex min-h-48 w-72 shrink-0 flex-col rounded-xl border bg-muted/30 transition-colors data-[over=true]:border-ring data-[over=true]:bg-muted/60"
          onDragOver={(e) => {
            if (!canMove) return;
            e.preventDefault();
            e.dataTransfer.dropEffect = 'move';
            setOver(column.status);
          }}
          onDragLeave={() => setOver((current) => (current === column.status ? null : current))}
          onDrop={(e) => {
            e.preventDefault();
            setOver(null);
            const id = e.dataTransfer.getData('text/plain');
            const project = projects.find((p) => p.id === id);
            if (project && project.status !== column.status) {
              move.mutate({ id, status: column.status });
            }
          }}
        >
          <div className="flex items-center gap-2 py-1.5 pr-1.5 pl-3">
            <h2 className="flex min-w-0 flex-1 items-center gap-2 text-[13px] font-medium">
              <ProjectStatusGlyph status={column.status} />
              <span className="truncate">{column.label}</span>
              <span className="text-xs font-normal text-muted-foreground">
                {column.projects.length}
              </span>
            </h2>
            {onCreate && (
              <IconButton
                label={`New project in ${column.label}`}
                onClick={() => onCreate(column.status)}
              >
                <PlusIcon />
              </IconButton>
            )}
          </div>
          <ul className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-2 pb-2">
            {column.projects.map((project) => (
              <li key={project.id}>
                <Link
                  to={`/app/projects/${project.id}`}
                  draggable={canMove}
                  onDragStart={(e) => {
                    e.dataTransfer.setData('text/plain', project.id);
                    e.dataTransfer.effectAllowed = 'move';
                  }}
                  className="block space-y-2 rounded-lg border bg-background p-2.5 text-[13px] shadow-xs outline-none hover:border-ring/60 focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/50 motion-safe:animate-fade-in"
                >
                  <span className="flex items-start gap-2">
                    <span className="line-clamp-2 min-w-0 flex-1 font-medium">{project.name}</span>
                    <PersonGlyph name={leadName(project.lead_id)} />
                  </span>
                  {(project.team || project.target_date) && (
                    <span className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                      {project.team && <TeamChip team={project.team} />}
                      {project.target_date && (
                        <span
                          title={`Target ${project.target_date}`}
                          className="flex items-center gap-1 tabular-nums"
                        >
                          <CalendarIcon aria-hidden className="size-3" />
                          {project.target_date}
                        </span>
                      )}
                    </span>
                  )}
                  <ProjectProgress project={project} />
                </Link>
              </li>
            ))}
            {column.projects.length === 0 && (
              <li className="px-1 py-2 text-xs text-muted-foreground/70">No projects</li>
            )}
          </ul>
        </section>
      ))}
    </ScrollRegion>
  );
}

function Projects({ workspace }: { workspace: Workspace }) {
  // The team is part of the address, so the sidebar's team rows and reloads land on it.
  const [params, setParams] = useSearchParams();
  const teamId = params.get('team') || ALL;
  const setTeamId = (id: string) => setParams(id === ALL ? {} : { team: id }, { replace: true });
  const [view, setView] = useState<View>(rememberedView);
  // The new-project dialog, with the status it starts in (a board column's).
  const [creating, setCreating] = useState<ProjectStatus | null>(null);
  const canEdit = workspace.role !== 'guest';
  const { data: teams = [] } = useQuery(teamsQuery(workspace.id));
  // Guests cannot list members; a lead then shows without a name.
  const { data: members = [] } = useQuery({ ...membersQuery(workspace.id), enabled: canEdit });
  const {
    data: projects,
    error,
    isPending,
    refetch,
  } = useQuery(projectsQuery(workspace.id, teamId === ALL ? undefined : teamId));
  const leadName: LeadName = (userId) => members.find((m) => m.user_id === userId)?.name;
  const team = teams.find((t) => t.id === teamId);
  const create = canEdit ? (status: ProjectStatus) => setCreating(status) : undefined;
  const chooseView = (next: View) => {
    setView(next);
    try {
      localStorage.setItem(VIEW_KEY, next);
    } catch {
      // The choice lasts for this visit only.
    }
  };
  const board = view === 'board';

  return (
    // A board fills the height under the header; its columns scroll on their own.
    <PageContainer className={board ? 'flex min-h-0 flex-1 flex-col' : undefined}>
      <PageHeader
        title="Projects"
        description={`Outcomes ${workspace.name} is working towards, and the teams that own them.`}
        actions={
          create && (
            <Button size="sm" onClick={() => create('planned')}>
              <PlusIcon />
              New project
            </Button>
          )
        }
      />
      <div className="flex flex-wrap items-center gap-2">
        <OptionSelect
          value={teamId}
          onValueChange={setTeamId}
          options={[
            { value: ALL, label: 'All teams' },
            ...teams.map((t) => ({ value: t.id, label: teamLabel(t) })),
          ]}
          aria-label="Team"
          className="w-52"
        />
        <div role="group" aria-label="View" className="ml-auto flex rounded-md border p-0.5">
          <IconButton
            variant={view === 'list' ? 'secondary' : 'ghost'}
            label="List view"
            aria-pressed={view === 'list'}
            onClick={() => chooseView('list')}
          >
            <ListIcon />
          </IconButton>
          <IconButton
            variant={board ? 'secondary' : 'ghost'}
            label="Board view"
            aria-pressed={board}
            onClick={() => chooseView('board')}
          >
            <KanbanIcon />
          </IconButton>
        </div>
      </div>
      {/* A list that could not be read is not an empty one; once read, it stays on screen when a
          later refetch fails (the toast says so). */}
      {!projects ? (
        isPending ? (
          <TableSkeleton rows={4} />
        ) : (
          <ErrorState
            title="Couldn’t load the projects"
            error={error}
            onRetry={() => void refetch()}
          />
        )
      ) : projects.length === 0 ? (
        teamId === ALL ? (
          <EmptyState
            icon={FolderKanbanIcon}
            title="No projects yet"
            description="A project groups issues around one outcome, such as a launch. A team can own it."
            action={
              create && (
                <Button onClick={() => create('planned')}>
                  <PlusIcon />
                  Name your first project
                </Button>
              )
            }
          />
        ) : (
          <EmptyState
            icon={FolderKanbanIcon}
            title={`No projects for ${team?.name ?? 'this team'}`}
            description="The projects this team owns are listed here. Create one, or give an existing project to the team from its page."
            action={
              <div className="flex flex-wrap justify-center gap-2">
                {create && <Button onClick={() => create('planned')}>New project</Button>}
                <Button variant="outline" onClick={() => setTeamId(ALL)}>
                  All teams
                </Button>
              </div>
            }
          />
        )
      ) : board ? (
        <ProjectBoard
          workspace={workspace}
          projects={projects}
          leadName={leadName}
          onCreate={create}
        />
      ) : (
        <ProjectList projects={projects} leadName={leadName} />
      )}
      {canEdit && (
        <NewProjectDialog
          key={creating ?? 'closed'}
          workspace={workspace}
          open={creating !== null}
          onClose={() => setCreating(null)}
          defaultTeam={teamId === ALL ? undefined : teamId}
          defaultStatus={creating ?? undefined}
        />
      )}
    </PageContainer>
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
