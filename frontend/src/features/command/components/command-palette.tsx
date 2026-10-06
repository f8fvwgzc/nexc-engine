import { useQuery } from '@tanstack/react-query';
import {
  ArrowUpRightIcon,
  CircleDotIcon,
  FileTextIcon,
  FolderKanbanIcon,
  KeyboardIcon,
  LogOutIcon,
  type LucideIcon,
  NetworkIcon,
  UserIcon,
  UsersIcon,
} from 'lucide-react';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { API_DOCS_ITEM, NAV_ITEMS } from '@/components/layout/nav-items';
import { THEME_OPTIONS } from '@/components/layout/theme-options';
import { KbdHint } from '@/components/custom-ui/kbd-hint';
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from '@/components/ui/command';
import { useLogout } from '@/features/auth/hooks/use-logout';
import { graphsQuery } from '@/features/graphs/api';
import { pageOf } from '@/features/insight/map-kinds';
import { SEARCH_MIN, workspaceSearchQuery, type SearchHit } from '@/features/search/api';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { useTheme } from '@/hooks/use-theme';
import { useCommandStore } from '@/stores/command-store';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';

/** What each kind of search hit is called and drawn with, in the order the groups are shown. */
const HIT_GROUPS: { kind: SearchHit['kind']; heading: string; icon: LucideIcon }[] = [
  { kind: 'issue', heading: 'Issues', icon: CircleDotIcon },
  { kind: 'project', heading: 'Projects', icon: FolderKanbanIcon },
  { kind: 'graph', heading: 'Graphs', icon: NetworkIcon },
  { kind: 'document', heading: 'Documents', icon: FileTextIcon },
  { kind: 'team', heading: 'Teams', icon: UsersIcon },
  { kind: 'member', heading: 'People', icon: UserIcon },
];

/**
 * ⌘K palette: search of the workspace (issues, projects, graphs, documents, teams, people),
 * navigation, page actions (canvas), theme and account.
 */
export function CommandPalette() {
  const open = useCommandStore((s) => s.paletteOpen);
  const setOpen = useCommandStore((s) => s.setPaletteOpen);
  const openShortcuts = useCommandStore((s) => s.setShortcutsOpen);
  const pageActions = useCommandStore((s) => s.pageActions);
  const navigate = useNavigate();
  const { setTheme } = useTheme();
  const logout = useLogout();
  const workspaceId = useWorkspaceId();
  const { data: graphs } = useQuery({ ...graphsQuery(workspaceId), enabled: open });
  const [input, setInput] = useState('');
  const term = useDebouncedValue(input.trim(), 200);
  const searching = term.length >= SEARCH_MIN;
  const search = workspaceSearchQuery(workspaceId, term);
  const { data: hits = [], isFetching } = useQuery({
    ...search,
    enabled: open && search.enabled,
  });

  const close = (next: boolean) => {
    setOpen(next);
    if (!next) setInput('');
  };
  const run = (fn: () => void) => {
    close(false);
    fn();
  };

  return (
    <CommandDialog open={open} onOpenChange={close} title="Command palette">
      <Command>
        <CommandInput
          value={input}
          onValueChange={setInput}
          placeholder="Search issues, projects, people… or type a command"
        />
        <CommandList>
          <CommandEmpty>
            {searching && isFetching ? 'Searching…' : 'No results found.'}
          </CommandEmpty>
          {searching &&
            HIT_GROUPS.map(({ kind, heading, icon: Icon }) => {
              const found = hits.filter((hit) => hit.kind === kind);
              if (found.length === 0) return null;
              return (
                <CommandGroup key={kind} heading={heading}>
                  {found.map((hit) => (
                    <CommandItem
                      key={hit.id}
                      // The server matched it; the typed text keeps the list's own filter from hiding it.
                      value={`${hit.title} ${hit.subtitle} ${hit.id} ${input}`}
                      onSelect={() => run(() => void navigate(pageOf(hit) ?? '/app'))}
                    >
                      <Icon />
                      <span className="truncate">{hit.title}</span>
                      {hit.subtitle && (
                        <span className="ml-auto max-w-[40%] shrink-0 truncate text-xs text-muted-foreground">
                          {hit.subtitle}
                        </span>
                      )}
                    </CommandItem>
                  ))}
                </CommandGroup>
              );
            })}
          {pageActions.length > 0 && (
            <>
              <CommandGroup heading="This graph">
                {pageActions.map((action) => (
                  <CommandItem
                    key={action.id}
                    value={action.label}
                    disabled={action.disabled}
                    onSelect={() => run(action.run)}
                  >
                    <action.icon />
                    {action.label}
                    {action.shortcut && (
                      <CommandShortcut>
                        <KbdHint keys={action.shortcut} />
                      </CommandShortcut>
                    )}
                  </CommandItem>
                ))}
              </CommandGroup>
              <CommandSeparator />
            </>
          )}
          <CommandGroup heading="Go to">
            {NAV_ITEMS.map((item) => (
              <CommandItem
                key={item.to}
                value={`Go to ${item.title}`}
                onSelect={() => run(() => void navigate(item.to))}
              >
                <item.icon />
                {item.title}
              </CommandItem>
            ))}
            <CommandItem
              value="Open API docs"
              onSelect={() =>
                run(() => window.open(API_DOCS_ITEM.to, '_blank', 'noopener,noreferrer'))
              }
            >
              <API_DOCS_ITEM.icon />
              API docs
              <ArrowUpRightIcon className="ml-auto opacity-50" />
            </CommandItem>
          </CommandGroup>
          {/* While searching, graphs come from the server with everything else. */}
          {!searching && graphs && graphs.length > 0 && (
            <CommandGroup heading="Graphs">
              {graphs.map((graph) => (
                <CommandItem
                  key={graph.id}
                  value={`Open graph ${graph.name} ${graph.id}`}
                  onSelect={() => run(() => void navigate(`/app/graphs/${graph.id}`))}
                >
                  <NetworkIcon />
                  <span className="truncate">{graph.name}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          <CommandSeparator />
          <CommandGroup heading="Preferences">
            {THEME_OPTIONS.map(({ value, label, icon: Icon }) => (
              <CommandItem
                key={value}
                value={`Theme ${label}`}
                onSelect={() => run(() => setTheme(value))}
              >
                <Icon />
                {label} theme
              </CommandItem>
            ))}
            <CommandItem value="Keyboard shortcuts" onSelect={() => run(() => openShortcuts(true))}>
              <KeyboardIcon />
              Keyboard shortcuts
              <CommandShortcut>?</CommandShortcut>
            </CommandItem>
            <CommandItem value="Log out" onSelect={() => run(() => logout.mutate())}>
              <LogOutIcon />
              Log out
            </CommandItem>
          </CommandGroup>
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
