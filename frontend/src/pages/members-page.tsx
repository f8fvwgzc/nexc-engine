import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from '@tanstack/react-query';
import { MailIcon, Trash2Icon, UserPlusIcon } from 'lucide-react';
import { Suspense, useState } from 'react';
import { toast } from 'sonner';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import {
  inviteMember,
  invitesQuery,
  membersQuery,
  removeMember,
  setMemberRole,
  withdrawInvite,
} from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';
import {
  isWorkspaceAdmin,
  type Workspace,
  type WorkspaceMember,
  type WorkspaceRole,
} from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';

const ROLE_LABEL: Record<WorkspaceRole, string> = {
  owner: 'Owner',
  admin: 'Admin',
  member: 'Member',
  guest: 'Guest',
};
const ROLE_HELP: Record<WorkspaceRole, string> = {
  owner: 'Full control, including deleting the workspace and naming other owners.',
  admin: 'Manages members, teams and workspace settings.',
  member: 'Works in every public team and can create teams.',
  guest: 'Sees only the teams they were added to.',
};

/** Roles `actor` may hand out: only owners touch ownership (the server enforces this). */
function assignableRoles(actor: WorkspaceRole): WorkspaceRole[] {
  if (actor === 'owner') return ['owner', 'admin', 'member', 'guest'];
  return actor === 'admin' ? ['admin', 'member', 'guest'] : [];
}

const roleOptions = (roles: WorkspaceRole[]) =>
  roles.map((value) => ({ value, label: ROLE_LABEL[value] }));

function InviteForm({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const [email, setEmail] = useState('');
  const [role, setRole] = useState<WorkspaceRole>('member');
  const invite = useMutation({
    mutationFn: () => inviteMember(workspace.id, { email: email.trim(), role }),
    onSuccess: (result) => {
      toast.success(
        result.member
          ? `${result.member.name} joined the workspace`
          : `Invitation saved — ${email.trim()} joins when they sign up`,
      );
      setEmail('');
      void queryClient.invalidateQueries({ queryKey: ['workspaces'] });
    },
  });
  return (
    <form
      className="flex flex-col gap-2 rounded-xl border p-3 sm:flex-row sm:items-center"
      onSubmit={(e) => {
        e.preventDefault();
        if (email.trim()) invite.mutate();
      }}
    >
      <Input
        type="email"
        value={email}
        placeholder="teammate@company.com"
        aria-label="E-mail address to invite"
        onChange={(e) => setEmail(e.target.value)}
        className="sm:flex-1"
      />
      <OptionSelect
        value={role}
        onValueChange={setRole}
        options={roleOptions(assignableRoles(workspace.role).filter((r) => r !== 'owner'))}
        aria-label="Role"
        className="sm:w-36"
      />
      <Button type="submit" disabled={!email.trim() || invite.isPending}>
        <UserPlusIcon />
        Invite
      </Button>
    </form>
  );
}

function PendingInvites({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const { data: invites = [] } = useQuery(invitesQuery(workspace.id));
  const withdraw = useMutation({
    mutationFn: (inviteId: string) => withdrawInvite(workspace.id, inviteId),
    meta: { successMessage: 'Invitation withdrawn' },
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: qk.workspaces.invites(workspace.id) }),
  });
  if (invites.length === 0) return null;
  return (
    <section aria-labelledby="pending-invites" className="space-y-2">
      <h2 id="pending-invites" className="text-sm font-medium">
        Pending invitations
      </h2>
      <ul className="divide-y rounded-xl border">
        {invites.map((invite) => (
          <li key={invite.id} className="flex items-center gap-3 px-3 py-2 text-sm">
            <MailIcon className="size-4 text-muted-foreground" aria-hidden />
            <span className="min-w-0 flex-1 truncate">{invite.email}</span>
            <Badge variant="secondary">{ROLE_LABEL[invite.role]}</Badge>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Withdraw the invitation of ${invite.email}`}
              onClick={() => withdraw.mutate(invite.id)}
            >
              <Trash2Icon />
            </Button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function MembersTable({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const { data: members } = useSuspenseQuery(membersQuery(workspace.id));
  const [removing, setRemoving] = useState<WorkspaceMember | null>(null);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['workspaces'] });
  const changeRole = useMutation({
    mutationFn: ({ userId, role }: { userId: string; role: WorkspaceRole }) =>
      setMemberRole(workspace.id, userId, role),
    meta: { successMessage: 'Role updated' },
    onSuccess: (list) => {
      queryClient.setQueryData(qk.workspaces.members(workspace.id), list);
      void refresh();
    },
  });
  const remove = useMutation({
    mutationFn: (userId: string) => removeMember(workspace.id, userId),
    meta: { successMessage: 'Member removed' },
    onSuccess: refresh,
  });
  const assignable = assignableRoles(workspace.role);

  return (
    <>
      <div className="overflow-x-auto rounded-xl border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>E-mail</TableHead>
              <TableHead className="w-40">Role</TableHead>
              <TableHead className="w-12" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {members.map((member) => {
              // Admins cannot change or remove owners; anyone may leave.
              const manageable = assignable.length > 0 && assignable.includes(member.role);
              const isMe = member.user_id === me;
              return (
                <TableRow key={member.user_id}>
                  <TableCell className="font-medium">
                    {member.name}
                    {isMe && <span className="ml-2 text-xs text-muted-foreground">you</span>}
                    {member.suspended && (
                      <Badge
                        variant="destructive"
                        className="ml-2"
                        title="Suspended by whoever administers this installation. They cannot sign in until that is lifted; it is not set in the workspace."
                      >
                        Suspended
                      </Badge>
                    )}
                  </TableCell>
                  <TableCell className="text-muted-foreground">{member.email}</TableCell>
                  <TableCell>
                    {manageable ? (
                      <OptionSelect
                        value={member.role}
                        onValueChange={(role) =>
                          role !== member.role &&
                          changeRole.mutate({ userId: member.user_id, role })
                        }
                        options={roleOptions(assignable)}
                        aria-label={`Role of ${member.name}`}
                      />
                    ) : (
                      <Badge variant="secondary" title={ROLE_HELP[member.role]}>
                        {ROLE_LABEL[member.role]}
                      </Badge>
                    )}
                  </TableCell>
                  <TableCell>
                    {(manageable || isMe) && (
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        className="text-destructive"
                        aria-label={isMe ? 'Leave the workspace' : `Remove ${member.name}`}
                        onClick={() => setRemoving(member)}
                      >
                        <Trash2Icon />
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title={
          removing?.user_id === me ? 'Leave this workspace?' : `Remove ${removing?.name ?? ''}?`
        }
        description="They lose access to the workspace and are removed from all of its teams."
        confirmLabel={removing?.user_id === me ? 'Leave' : 'Remove'}
        destructive
        onConfirm={() => removing && remove.mutate(removing.user_id)}
      />
    </>
  );
}

function Members() {
  const { current } = useCurrentWorkspace();
  if (!current) return <PageSkeleton />;
  const admin = isWorkspaceAdmin(current.role);
  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <PageHeader
        title="Members"
        description={`Who belongs to ${current.name} and what they may do. You are ${
          current.role === 'owner' || current.role === 'admin' ? 'an' : 'a'
        } ${current.role}.`}
      />
      {current.role === 'guest' ? (
        <p className="rounded-xl border p-4 text-sm text-muted-foreground">
          Guests work in the teams they were added to and cannot see the workspace's members.
        </p>
      ) : (
        <>
          {admin && <InviteForm workspace={current} />}
          <Suspense fallback={<PageSkeleton />}>
            <MembersTable key={current.id} workspace={current} />
          </Suspense>
          {admin && <PendingInvites workspace={current} />}
          <dl className="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-[auto_1fr]">
            {(Object.keys(ROLE_LABEL) as WorkspaceRole[]).map((role) => (
              <div key={role} className="contents">
                <dt className="font-medium">{ROLE_LABEL[role]}</dt>
                <dd className="text-muted-foreground">{ROLE_HELP[role]}</dd>
              </div>
            ))}
          </dl>
        </>
      )}
    </div>
  );
}

export default function MembersPage() {
  return (
    <>
      <Seo title="Members" noIndex />
      <Members />
    </>
  );
}
