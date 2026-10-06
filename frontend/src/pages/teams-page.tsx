import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from '@tanstack/react-query';
import {
  LockIcon,
  LogInIcon,
  LogOutIcon,
  PlusIcon,
  RefreshCwIcon,
  Trash2Icon,
  UsersRoundIcon,
  WorkflowIcon,
} from 'lucide-react';
import { Suspense, useState } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import {
  createTeam,
  deleteTeam,
  membersQuery,
  removeTeamMember,
  setTeamMember,
  teamMembersQuery,
  teamsQuery,
} from '@/features/workspaces/api';
import { CyclesDialog } from '@/features/issues/cycles-dialog';
import { WorkflowDialog } from '@/features/issues/workflow-dialog';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import { isWorkspaceAdmin, type Team, type Workspace } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';

function NewTeamDialog({
  workspace,
  open,
  onClose,
}: {
  workspace: Workspace;
  open: boolean;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const [key, setKey] = useState('');
  const [description, setDescription] = useState('');
  const [isPrivate, setPrivate] = useState(false);
  const create = useMutation({
    mutationFn: () =>
      createTeam(workspace.id, {
        name: name.trim(),
        key: key.trim() || undefined,
        description: description.trim(),
        private: isPrivate,
      }),
    meta: { errorToast: false, successMessage: 'Team created' },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: qk.workspaces.teams(workspace.id) });
      setName('');
      setKey('');
      setDescription('');
      setPrivate(false);
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-md">
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) create.mutate();
          }}
        >
          <DialogHeader>
            <DialogTitle>New team</DialogTitle>
            <DialogDescription>
              Teams group people and their work. You become the team's owner.
            </DialogDescription>
          </DialogHeader>
          <div className="grid grid-cols-[1fr_7rem] gap-2">
            <Input
              autoFocus
              value={name}
              maxLength={80}
              placeholder="Core Platform"
              aria-label="Team name"
              onChange={(e) => setName(e.target.value)}
            />
            <Input
              value={key}
              maxLength={7}
              placeholder="Key (CP)"
              aria-label="Team key"
              onChange={(e) => setKey(e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, ''))}
              className="font-mono"
            />
          </div>
          <Input
            value={description}
            maxLength={500}
            placeholder="What this team works on (optional)"
            aria-label="Description"
            onChange={(e) => setDescription(e.target.value)}
          />
          <label className="flex items-center gap-2 text-sm">
            <Switch checked={isPrivate} onCheckedChange={setPrivate} />
            Private — only its members can see it
          </label>
          {create.error && (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(create.error)}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!name.trim() || create.isPending}>
              Create team
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Members of one team, with the controls its owners and workspace admins get. */
function TeamRoster({
  workspace,
  team,
  canManage,
}: {
  workspace: Workspace;
  team: Team;
  canManage: boolean;
}) {
  const queryClient = useQueryClient();
  const { data: members = [] } = useQuery(teamMembersQuery(workspace.id, team.id));
  // Guests cannot list workspace members; they never manage a team either.
  const { data: everyone = [] } = useQuery({
    ...membersQuery(workspace.id),
    enabled: canManage,
  });
  const [adding, setAdding] = useState('');
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: qk.workspaces.teams(workspace.id) });
  const add = useMutation({
    mutationFn: (userId: string) => setTeamMember(workspace.id, team.id, userId),
    meta: { successMessage: 'Added to the team' },
    onSuccess: () => {
      setAdding('');
      void refresh();
    },
  });
  const remove = useMutation({
    mutationFn: (userId: string) => removeTeamMember(workspace.id, team.id, userId),
    meta: { successMessage: 'Removed from the team' },
    onSuccess: refresh,
  });
  const candidates = everyone
    .filter((m) => !members.some((tm) => tm.user_id === m.user_id))
    .map((m) => ({ value: m.user_id, label: `${m.name} · ${m.role}` }));

  return (
    <div className="space-y-2 border-t pt-3">
      <ul className="space-y-1">
        {members.map((member) => (
          <li key={member.user_id} className="flex items-center gap-2 text-sm">
            <span className="min-w-0 flex-1 truncate">
              {member.name} <span className="text-muted-foreground">· {member.email}</span>
            </span>
            {member.role === 'owner' && <Badge variant="secondary">Owner</Badge>}
            {canManage && (
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Remove ${member.name} from ${team.name}`}
                onClick={() => remove.mutate(member.user_id)}
              >
                <Trash2Icon />
              </Button>
            )}
          </li>
        ))}
        {members.length === 0 && <li className="text-sm text-muted-foreground">No members yet.</li>}
      </ul>
      {canManage && candidates.length > 0 && (
        <div className="flex items-center gap-2">
          <OptionSelect
            value={adding}
            onValueChange={setAdding}
            options={candidates}
            placeholder="Add a workspace member…"
            aria-label={`Add a member to ${team.name}`}
          />
          <Button
            variant="outline"
            disabled={!adding || add.isPending}
            onClick={() => add.mutate(adding)}
          >
            Add
          </Button>
        </div>
      )}
    </div>
  );
}

function TeamCard({ workspace, team }: { workspace: Workspace; team: Team }) {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const [open, setOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [workflowOpen, setWorkflowOpen] = useState(false);
  const [cyclesOpen, setCyclesOpen] = useState(false);
  const canManage = team.role === 'owner' || isWorkspaceAdmin(workspace.role);
  const canJoin = team.role === null && !team.private && workspace.role !== 'guest';
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: qk.workspaces.teams(workspace.id) });
  const join = useMutation({
    mutationFn: () => setTeamMember(workspace.id, team.id, me ?? ''),
    meta: { successMessage: `Joined ${team.name}` },
    onSuccess: refresh,
  });
  const leave = useMutation({
    mutationFn: () => removeTeamMember(workspace.id, team.id, me ?? ''),
    meta: { successMessage: `Left ${team.name}` },
    onSuccess: refresh,
  });
  const remove = useMutation({
    mutationFn: () => deleteTeam(workspace.id, team.id),
    meta: { successMessage: 'Team deleted' },
    onSuccess: refresh,
  });

  return (
    <li className="space-y-3 rounded-xl border p-4">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline" className="font-mono">
          {team.key}
        </Badge>
        <h2 className="min-w-0 flex-1 truncate font-medium">{team.name}</h2>
        {team.private && (
          <Badge variant="secondary">
            <LockIcon aria-hidden />
            Private
          </Badge>
        )}
        {team.role && <Badge>{team.role === 'owner' ? 'Team owner' : 'Member'}</Badge>}
      </div>
      {team.description && <p className="text-sm text-muted-foreground">{team.description}</p>}
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="ghost" size="sm" onClick={() => setOpen((v) => !v)} aria-expanded={open}>
          <UsersRoundIcon />
          {team.member_count === 1 ? '1 member' : `${team.member_count} members`}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => setWorkflowOpen(true)}>
          <WorkflowIcon />
          Workflow
        </Button>
        <Button variant="ghost" size="sm" onClick={() => setCyclesOpen(true)}>
          <RefreshCwIcon />
          Cycles
        </Button>
        <span className="flex-1" />
        {canJoin && (
          <Button
            variant="outline"
            size="sm"
            disabled={join.isPending}
            onClick={() => join.mutate()}
          >
            <LogInIcon />
            Join
          </Button>
        )}
        {team.role && (
          <Button
            variant="ghost"
            size="sm"
            disabled={leave.isPending}
            onClick={() => leave.mutate()}
          >
            <LogOutIcon />
            Leave
          </Button>
        )}
        {canManage && (
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-destructive"
            aria-label={`Delete ${team.name}`}
            onClick={() => setDeleting(true)}
          >
            <Trash2Icon />
          </Button>
        )}
      </div>
      {open && <TeamRoster workspace={workspace} team={team} canManage={canManage} />}
      <WorkflowDialog
        workspace={workspace}
        team={team}
        editable={canManage}
        open={workflowOpen}
        onClose={() => setWorkflowOpen(false)}
      />
      <CyclesDialog
        workspace={workspace}
        team={team}
        editable={canManage}
        open={cyclesOpen}
        onClose={() => setCyclesOpen(false)}
      />
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={`Delete ${team.name}?`}
        description="The team and its memberships are removed. A team that still has graphs cannot be deleted."
        confirmLabel="Delete team"
        destructive
        onConfirm={() => remove.mutate()}
      />
    </li>
  );
}

function TeamList({ workspace, onCreate }: { workspace: Workspace; onCreate?: () => void }) {
  const { data: teams } = useSuspenseQuery(teamsQuery(workspace.id));
  if (teams.length === 0) {
    return (
      <EmptyState
        icon={UsersRoundIcon}
        title="No teams yet"
        description={
          onCreate
            ? 'Create a team for each group that works together, such as Engineering or Research.'
            : 'You have not been added to a team in this workspace yet.'
        }
        action={onCreate && <Button onClick={onCreate}>Create your first team</Button>}
      />
    );
  }
  return (
    <ul className="grid gap-3 md:grid-cols-2">
      {teams.map((team) => (
        <TeamCard key={team.id} workspace={workspace} team={team} />
      ))}
    </ul>
  );
}

function Teams() {
  const { current } = useCurrentWorkspace();
  const [creating, setCreating] = useState(false);
  if (!current) return <PageSkeleton />;
  const canCreate = current.role !== 'guest';
  return (
    <div className="mx-auto w-full max-w-5xl space-y-6 p-4 sm:p-6">
      <PageHeader
        title="Teams"
        description={`The teams of ${current.name}. Public teams are open to every member; private teams only to the people in them.`}
        actions={
          canCreate && (
            <Button onClick={() => setCreating(true)}>
              <PlusIcon />
              New team
            </Button>
          )
        }
      />
      <Suspense fallback={<PageSkeleton />}>
        <TeamList
          key={current.id}
          workspace={current}
          onCreate={canCreate ? () => setCreating(true) : undefined}
        />
      </Suspense>
      <NewTeamDialog workspace={current} open={creating} onClose={() => setCreating(false)} />
    </div>
  );
}

export default function TeamsPage() {
  return (
    <>
      <Seo title="Teams" noIndex />
      <Teams />
    </>
  );
}
