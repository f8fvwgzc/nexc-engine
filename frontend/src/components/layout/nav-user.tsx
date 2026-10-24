import { CodeXmlIcon, KeyboardIcon, LogOutIcon, PaletteIcon, UserIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { IconButton } from '@/components/custom-ui/icon-button';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useLogout } from '@/features/auth/hooks/use-logout';
import { useTheme } from '@/hooks/use-theme';
import { useAuthStore } from '@/stores/auth-store';
import { useCommandStore } from '@/stores/command-store';

import { API_DOCS_ITEM, DOCS_ITEM } from './nav-items';
import { isTheme, THEME_OPTIONS } from './theme-options';

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  const letters = parts.length > 1 ? [parts[0], parts.at(-1)] : [parts[0]];
  return (
    letters
      .map((p) => p?.[0] ?? '')
      .join('')
      .toUpperCase() || '?'
  );
}

/** The account menu, behind the avatar at the bottom of the rail. */
export function NavUser() {
  const user = useAuthStore((s) => s.user);
  const { theme, setTheme } = useTheme();
  const logout = useLogout();
  const openShortcuts = useCommandStore((s) => s.setShortcutsOpen);
  if (!user) return null;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <IconButton
          label="Account menu"
          tooltipSide="right"
          size="icon-lg"
          className="rounded-lg text-sidebar-foreground hover:bg-sidebar-accent data-[state=open]:bg-sidebar-accent"
        >
          <Avatar className="size-7">
            <AvatarFallback className="bg-primary text-[11px] font-semibold text-primary-foreground">
              {initials(user.name)}
            </AvatarFallback>
          </Avatar>
        </IconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent className="min-w-60 rounded-lg" side="right" align="end" sideOffset={8}>
        <DropdownMenuLabel className="p-0 font-normal">
          <div className="flex items-center gap-2 px-1 py-1.5">
            <Avatar className="size-8 rounded-lg">
              <AvatarFallback className="rounded-lg bg-primary text-xs font-semibold text-primary-foreground">
                {initials(user.name)}
              </AvatarFallback>
            </Avatar>
            <div className="grid flex-1 text-left text-sm leading-tight">
              <span className="truncate font-medium">{user.name}</span>
              <span className="truncate text-xs text-muted-foreground">{user.email}</span>
            </div>
          </div>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          {/* The same page, inside whichever layout the account works in. */}
          <DropdownMenuItem asChild>
            <Link to={user.role === 'admin' ? '/app/platform/account' : '/app/settings/profile'}>
              <UserIcon />
              Profile
            </Link>
          </DropdownMenuItem>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              <PaletteIcon />
              Theme
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              <DropdownMenuRadioGroup
                value={theme}
                onValueChange={(v) => isTheme(v) && setTheme(v)}
              >
                {THEME_OPTIONS.map(({ value, label, icon: Icon }) => (
                  <DropdownMenuRadioItem key={value} value={value}>
                    <Icon />
                    {label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuItem onSelect={() => openShortcuts(true)}>
            <KeyboardIcon />
            Keyboard shortcuts
          </DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        {/* Help: the documentation site and this server's API reference, each in a new tab. */}
        <DropdownMenuGroup>
          <DropdownMenuItem asChild>
            <a href={DOCS_ITEM.to} target="_blank" rel="noopener noreferrer">
              <DOCS_ITEM.icon />
              {DOCS_ITEM.title}
            </a>
          </DropdownMenuItem>
          <DropdownMenuItem asChild>
            <a href={API_DOCS_ITEM.to} target="_blank" rel="noopener noreferrer">
              <CodeXmlIcon />
              API reference
            </a>
          </DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          variant="destructive"
          disabled={logout.isPending}
          onSelect={() => logout.mutate()}
        >
          <LogOutIcon />
          Log out
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
