import { useQuery } from '@tanstack/react-query';
import { ArrowUpRightIcon, KeyboardIcon, LogOutIcon, NetworkIcon } from 'lucide-react';
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
import { useTheme } from '@/hooks/use-theme';
import { useCommandStore } from '@/stores/command-store';
import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';

/** ⌘K palette: navigation, graphs, page actions (canvas), theme and account. */
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

  const run = (fn: () => void) => {
    setOpen(false);
    fn();
  };

  return (
    <CommandDialog open={open} onOpenChange={setOpen} title="Command palette">
      <Command>
        <CommandInput placeholder="Type a command or search graphs…" />
        <CommandList>
          <CommandEmpty>No results found.</CommandEmpty>
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
          {graphs && graphs.length > 0 && (
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
