import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

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
import { Textarea } from '@/components/ui/textarea';
import { membersQuery, teamsQuery } from '@/features/workspaces/api';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { ProjectStatus } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';

import { createProject } from './api';
import { ProjectLeadSelect, ProjectStatusSelect, ProjectTeamSelect } from './project-fields';

/**
 * Names a new project; its team, status, lead, target date and summary are optional. Opened from
 * a team or a board column, it starts with that team or status chosen. The creator leads unless
 * another lead, or none, is picked.
 */
export function NewProjectDialog({
  workspace,
  open,
  onClose,
  defaultTeam,
  defaultStatus = 'planned',
}: {
  workspace: Workspace;
  open: boolean;
  onClose: () => void;
  defaultTeam?: string;
  defaultStatus?: ProjectStatus;
}) {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const { data: teams = [] } = useQuery(teamsQuery(workspace.id));
  // Guests cannot list members; they cannot create projects either.
  const { data: members = [] } = useQuery({
    ...membersQuery(workspace.id),
    enabled: workspace.role !== 'guest',
  });
  const [name, setName] = useState('');
  const [teamId, setTeamId] = useState<string | null>(defaultTeam ?? null);
  const [status, setStatus] = useState<ProjectStatus>(defaultStatus);
  const [leadId, setLeadId] = useState<string | null>(me ?? null);
  const [targetDate, setTargetDate] = useState('');
  const [description, setDescription] = useState('');
  const create = useMutation({
    mutationFn: () =>
      createProject(workspace.id, {
        name: name.trim(),
        description: description.trim() || undefined,
        team_id: teamId ?? undefined,
        status,
        // Sent as null for "No lead": left out, the backend makes the creator the lead.
        lead_id: leadId,
        target_date: targetDate || undefined,
      }),
    meta: { errorToast: false, successMessage: 'Project created' },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: qk.issues.projectLists(workspace.id) });
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) create.mutate();
          }}
        >
          <DialogHeader>
            <DialogTitle>New project</DialogTitle>
            <DialogDescription>
              A project gathers issues around one outcome, such as a launch. A team can own it.
            </DialogDescription>
          </DialogHeader>
          <Input
            autoFocus
            value={name}
            maxLength={200}
            placeholder="Project name"
            aria-label="Name"
            onChange={(e) => setName(e.target.value)}
          />
          <div className="grid gap-2 sm:grid-cols-3">
            <ProjectTeamSelect teams={teams} value={teamId} onValueChange={setTeamId} />
            <ProjectStatusSelect value={status} onValueChange={setStatus} />
            <ProjectLeadSelect members={members} value={leadId} onValueChange={setLeadId} />
          </div>
          <Textarea
            value={description}
            rows={3}
            placeholder="What the project should achieve (optional)"
            aria-label="Description"
            onChange={(e) => setDescription(e.target.value)}
          />
          <label className="flex items-center gap-2 text-[13px] text-muted-foreground">
            Target
            <Input
              type="date"
              value={targetDate}
              aria-label="Target date"
              className="h-8 w-40 text-[13px] text-foreground"
              onChange={(e) => setTargetDate(e.target.value)}
            />
            <span className="text-xs">optional</span>
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
              Create project
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
