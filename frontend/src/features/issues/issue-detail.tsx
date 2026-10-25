import { type QueryClient, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { PanelRightOpenIcon } from 'lucide-react';
import { useEffect, useRef, useState, type ReactNode } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { IconButton } from '@/components/custom-ui/icon-button';
import { Skeleton } from '@/components/ui/skeleton';
import { Textarea } from '@/components/ui/textarea';
import { teamsQuery } from '@/features/workspaces/api';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { Issue, IssueInput } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { deleteIssue, updateIssue, type IssueFilter } from './api';
import { IssueProperties, PropertiesSkeleton } from './issue-properties';
import { IssueRun } from './issue-run';
import { IssueTimeline } from './issue-timeline';
import { useLeaveIssue } from './links';
import { SubIssues } from './sub-issues';

/** Where the member's choice to show or hide the properties panel is kept, for every issue. */
const PANEL_KEY = 'nexc.issue.properties';

/** Whether the properties panel is shown: it is, unless the member closed it. */
function readPanelOpen(): boolean {
  try {
    return localStorage.getItem(PANEL_KEY) !== 'closed';
  } catch {
    return true;
  }
}

function writePanelOpen(open: boolean) {
  try {
    localStorage.setItem(PANEL_KEY, open ? 'open' : 'closed');
  } catch {
    // The choice lasts for this visit only.
  }
}

/**
 * Shows a copy of the issue just saved: on its page at once, then read again where the change
 * shows: the workspace's issue lists (not its own sub-issues, which do not carry it), its
 * timeline, the inbox when it was assigned or moved, its parent's count of closed parts, and the
 * counts of projects and cycles (only marked stale: nothing on this page shows them). The issue
 * itself is not read again: the copy the server just answered with is the newest.
 */
function showSaved(queryClient: QueryClient, updated: Issue, body: Partial<IssueInput>) {
  const ws = updated.workspace_id;
  queryClient.setQueryData(qk.issues.one(updated.id), updated);
  void queryClient.invalidateQueries({
    queryKey: qk.issues.lists(ws),
    predicate: (query) => (query.queryKey[3] as IssueFilter | undefined)?.parent_id !== updated.id,
  });
  void queryClient.invalidateQueries({ queryKey: qk.issues.events(updated.id) });
  const moved = 'state_id' in body;
  if (moved || 'assignee_id' in body) {
    void queryClient.invalidateQueries({ queryKey: qk.issues.inbox(ws) });
  }
  if (moved && updated.parent) {
    void queryClient.invalidateQueries({ queryKey: qk.issues.one(updated.parent.id) });
  }
  if (moved || 'project_id' in body) {
    void queryClient.invalidateQueries({
      queryKey: qk.issues.projectLists(ws),
      refetchType: 'none',
    });
  }
  if (moved || 'cycle_id' in body) {
    void queryClient.invalidateQueries({
      queryKey: qk.issues.cycles(ws, updated.team_id),
      refetchType: 'none',
    });
  }
}

/**
 * The title or the description, edited in place. The field shows the issue as last read until
 * the member types; what they type is kept until it is saved, when the field is left. Leaving
 * sends that one field, and only if it differs from what the field showed when they began typing.
 * So a field that is only focused and left sends nothing, and a newer copy of the issue (a
 * teammate's or an agent's change, or the fresh read that replaces a list's copy) is never written
 * over with an older one.
 */
function useInlineText(issue: Issue, field: 'title' | 'description') {
  const queryClient = useQueryClient();
  const value = issue[field];
  // A title is saved trimmed and is never blank; a description is saved as typed.
  const clean = (text: string) => (field === 'title' ? text.trim() : text);
  // What the member typed, and the issue's value when they began.
  const [draft, setDraft] = useState<{ from: string; text: string } | null>(null);
  const save = useMutation({
    mutationFn: (text: string) =>
      updateIssue(issue.id, field === 'title' ? { title: text } : { description: text }),
    onSuccess: (updated, text) => {
      showSaved(queryClient, updated, field === 'title' ? { title: text } : { description: text });
      // Typing that went on while it saved stays, measured from what was saved.
      setDraft((d) => (d && clean(d.text) !== text ? { ...d, from: text } : null));
    },
  });
  return {
    value: draft?.text ?? value,
    error: save.error,
    onChange: (text: string) => setDraft((d) => ({ from: d?.from ?? value, text })),
    // A save that fails keeps the text, and leaving the field again tries again.
    commit: () => {
      if (!draft) return;
      const text = clean(draft.text);
      if (text === draft.from || (field === 'title' && !text)) setDraft(null);
      else if (!(save.isPending && save.variables === text)) save.mutate(text);
    },
  };
}

/**
 * Everything about one issue, editable in place, laid out like Linear: a main column that scrolls
 * (title, description, the agents' work, sub-issues, files, activity), its sections set apart by
 * space rather than lines, beside a properties panel with its own scroll that the member can
 * close (and open again from the top of the main column); on a phone the panel follows the main
 * column. It fills the height its page gives it (`flex-1` in a flex column) from `md` up, and
 * fades in where the skeleton was. `descriptionPending` holds the description back (a skeleton
 * line) while the issue shown is a list's copy, which has none.
 */
export function IssueDetail({
  workspace,
  issue,
  descriptionPending = false,
  artifacts,
}: {
  workspace: Workspace;
  issue: Issue;
  descriptionPending?: boolean;
  /** The issue's files, shown between its sub-issues and its activity. */
  artifacts?: ReactNode;
}) {
  const queryClient = useQueryClient();
  const leave = useLeaveIssue();
  const title = useInlineText(issue, 'title');
  const description = useInlineText(issue, 'description');
  const [confirming, setConfirming] = useState(false);
  const [panelOpen, setPanelOpen] = useState(readPanelOpen);
  const openButton = useRef<HTMLButtonElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  // Set when the member shows or hides the panel, whose button then leaves the page.
  const refocus = useRef(false);
  const showPanel = (open: boolean) => {
    refocus.current = true;
    setPanelOpen(open);
    writePanelOpen(open);
  };
  // Focus moves to the button that undoes it, instead of falling back to the top of the page.
  useEffect(() => {
    if (!refocus.current) return;
    refocus.current = false;
    (panelOpen ? closeButton : openButton).current?.focus();
  }, [panelOpen]);
  // Fills the cache the header's team crumb reads; the page itself does not name the team.
  useQuery(teamsQuery(workspace.id));
  const save = useMutation({
    mutationFn: (body: Partial<IssueInput>) => updateIssue(issue.id, body),
    onSuccess: (updated, body) => showSaved(queryClient, updated, body),
  });
  const remove = useMutation({
    mutationFn: () => deleteIssue(issue.id),
    meta: { successMessage: 'Issue deleted' },
    onSuccess: () => {
      leave();
      // Where it was counted or listed is read again where it shows next; the deleted issue's
      // own entries just go, and its parts' copies (which name it as their parent) go stale.
      const ws = issue.workspace_id;
      void queryClient.invalidateQueries({ queryKey: qk.issues.lists(ws) });
      void queryClient.invalidateQueries({ queryKey: qk.issues.inbox(ws) });
      void queryClient.invalidateQueries({
        queryKey: qk.issues.all,
        predicate: (query) => query.queryKey[1] === 'detail' && !query.queryKey.includes(issue.id),
        refetchType: 'none',
      });
      for (const queryKey of [qk.issues.projectLists(ws), qk.issues.cycles(ws, issue.team_id)]) {
        void queryClient.invalidateQueries({ queryKey, refetchType: 'none' });
      }
      // Its files went with it (a private team's) or lost their issue: every file list and file
      // is read again where it is next shown, not here on the way out.
      void queryClient.invalidateQueries({ queryKey: qk.artifacts.all, refetchType: 'none' });
    },
  });
  const saveError = title.error ?? description.error ?? save.error;

  return (
    <div className="flex flex-col motion-safe:animate-fade-in md:min-h-0 md:flex-1">
      <h1 className="sr-only">
        {issue.identifier} {issue.title}
      </h1>
      <div className="flex flex-col md:min-h-0 md:flex-1 md:flex-row">
        <div className="min-w-0 flex-1 md:overflow-y-auto">
          <div className="w-full space-y-10 px-4 py-6 sm:px-8">
            <div className="space-y-2">
              <div className="flex items-start gap-2">
                {/* Text saves when the field is left, like every property saves when it is picked. */}
                <textarea
                  value={title.value}
                  rows={1}
                  maxLength={200}
                  aria-label="Title"
                  placeholder="Issue title"
                  className="field-sizing-content min-w-0 flex-1 resize-none bg-transparent text-xl font-semibold tracking-tight outline-none placeholder:text-muted-foreground"
                  onChange={(e) => title.onChange(e.target.value)}
                  onBlur={title.commit}
                  onKeyDown={(e) => {
                    // The Enter that ends an IME composition only confirms the text.
                    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      e.currentTarget.blur();
                    }
                  }}
                />
                {!panelOpen && (
                  <IconButton
                    ref={openButton}
                    label="Open properties"
                    className="text-muted-foreground hover:text-foreground"
                    onClick={() => showPanel(true)}
                  >
                    <PanelRightOpenIcon />
                  </IconButton>
                )}
              </div>
              {descriptionPending ? (
                // A list's copy has no description: nothing to edit (or to save) until it lands.
                <div role="status" aria-label="Loading the description" className="space-y-2 py-2">
                  <Skeleton className="h-4 w-5/6" />
                  <Skeleton className="h-4 w-1/2" />
                </div>
              ) : (
                <Textarea
                  value={description.value}
                  aria-label="Description"
                  placeholder="Add a description. Agents work from it when the issue is handed to them."
                  className="min-h-24 resize-none rounded-none border-0 bg-transparent px-0 shadow-none focus-visible:ring-0 dark:bg-transparent"
                  onChange={(e) => description.onChange(e.target.value)}
                  onBlur={description.commit}
                />
              )}
              {saveError && (
                <p role="alert" className="text-sm text-destructive">
                  {errorMessage(saveError)}
                </p>
              )}
            </div>
            <IssueRun issue={issue} />
            <SubIssues workspace={workspace} issue={issue} />
            {artifacts}
            <IssueTimeline
              issue={issue}
              canModerate={workspace.role === 'owner' || workspace.role === 'admin'}
            />
          </div>
        </div>
        <IssueProperties
          workspace={workspace}
          issue={issue}
          open={panelOpen}
          closeRef={closeButton}
          onClose={() => showPanel(false)}
          save={(body) => save.mutate(body)}
          onSaved={(updated, body) => showSaved(queryClient, updated, body)}
          onDelete={() => setConfirming(true)}
        />
      </div>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title={`Delete ${issue.identifier}?`}
        description="The issue is removed. A graph created for it stays."
        confirmLabel="Delete issue"
        destructive
        onConfirm={() => remove.mutate()}
      />
    </div>
  );
}

/**
 * The issue page's frame while the issue loads, laid out like the page: the main column and,
 * unless the member closed it, the properties panel.
 */
export function IssueDetailSkeleton() {
  const [panelOpen] = useState(readPanelOpen);
  return (
    <div
      role="status"
      aria-busy
      aria-label="Loading the issue"
      className="flex flex-col md:min-h-0 md:flex-1 md:flex-row"
    >
      <div className="w-full min-w-0 flex-1 space-y-10 px-4 py-6 sm:px-8">
        <div className="space-y-3">
          <Skeleton className="h-7 w-2/3" />
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-5/6" />
        </div>
        <div className="space-y-2">
          <Skeleton className="h-3.5 w-24" />
          <Skeleton className="h-8 w-full" />
        </div>
      </div>
      {panelOpen && <PropertiesSkeleton />}
    </div>
  );
}
