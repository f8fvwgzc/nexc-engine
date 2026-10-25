import { useQuery } from '@tanstack/react-query';
import { ChevronRightIcon, FolderIcon, FolderMinusIcon } from 'lucide-react';
import { useState } from 'react';
import { Link, useLocation } from 'react-router-dom';

import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
} from '@/components/ui/sidebar';
import { artifactFoldersQuery } from '@/features/artifacts/api';
import { buildFolderTree, folderPath, type FolderNode } from '@/features/artifacts/files';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { prefetchRouteOnIntent } from '@/lib/prefetch';
import { cn } from '@/lib/utils';

import { ROW } from './nav-section';

/** How far each level of the tree is indented. */
const INDENT_REM = 0.75;

interface FolderRowsProps {
  nodes: FolderNode[];
  depth: number;
  open: string | null;
  isExpanded: (id: string) => boolean;
  onToggle: (id: string, expanded: boolean) => void;
}

/** One level of the tree: each folder a link, with a chevron that folds what is under it. */
function FolderRows({ nodes, depth, open, isExpanded, onToggle }: FolderRowsProps) {
  return nodes.map(({ folder, children }) => {
    const expanded = children.length > 0 && isExpanded(folder.id);
    return (
      <SidebarMenuItem key={folder.id}>
        <div className="flex items-center" style={{ paddingLeft: `${depth * INDENT_REM}rem` }}>
          {children.length > 0 ? (
            <button
              type="button"
              aria-expanded={expanded}
              aria-label={`${expanded ? 'Hide' : 'Show'} the folders in ${folder.name}`}
              onClick={() => onToggle(folder.id, !expanded)}
              className="flex size-[24px] shrink-0 items-center justify-center rounded-sm text-sidebar-foreground/60 ring-sidebar-ring outline-hidden hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2"
            >
              <ChevronRightIcon
                className={cn('size-3.5 transition-transform', expanded && 'rotate-90')}
              />
            </button>
          ) : (
            <span aria-hidden className="w-[24px] shrink-0" />
          )}
          <SidebarMenuButton
            asChild
            isActive={open === folder.id}
            className={cn(ROW, 'min-w-0 flex-1 pl-1')}
          >
            <Link
              to={`/app/artifacts?folder=${folder.id}`}
              aria-current={open === folder.id ? 'page' : undefined}
              {...prefetchRouteOnIntent('/app/artifacts')}
            >
              <FolderIcon className="text-muted-foreground" />
              <span>{folder.name}</span>
            </Link>
          </SidebarMenuButton>
        </div>
        {expanded && (
          <SidebarMenu className="gap-0.5">
            <FolderRows
              nodes={children}
              depth={depth + 1}
              open={open}
              isExpanded={isExpanded}
              onToggle={onToggle}
            />
          </SidebarMenu>
        )}
      </SidebarMenuItem>
    );
  });
}

/**
 * The artifacts panel's folder tree: each folder opens its files, and a folder with folders in
 * it folds. The folders above the open one start unfolded. Guests see no folders.
 */
export function NavArtifactFolders() {
  const { current } = useCurrentWorkspace();
  const { pathname, search } = useLocation();
  const guest = current?.role === 'guest';
  const { data: folders = [], isPending } = useQuery({
    ...artifactFoldersQuery(current?.id ?? ''),
    enabled: Boolean(current) && !guest,
  });
  // What the member folded or unfolded; the rest follows the open folder.
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  if (!current || guest) return null;
  const open = pathname === '/app/artifacts' ? new URLSearchParams(search).get('folder') : null;
  const above = new Set(open ? folderPath(folders, open).map((f) => f.id) : []);
  const isExpanded = (id: string) => toggled[id] ?? above.has(id);
  return (
    <SidebarGroup className="py-1">
      <SidebarGroupLabel className="h-7">Folders</SidebarGroupLabel>
      <SidebarMenu className="gap-0.5">
        <SidebarMenuItem>
          <div className="flex items-center">
            <span aria-hidden className="w-[24px] shrink-0" />
            <SidebarMenuButton
              asChild
              isActive={open === 'root'}
              className={cn(ROW, 'min-w-0 flex-1 pl-1')}
            >
              <Link
                to="/app/artifacts?folder=root"
                aria-current={open === 'root' ? 'page' : undefined}
                {...prefetchRouteOnIntent('/app/artifacts')}
              >
                <FolderMinusIcon className="text-muted-foreground" />
                <span>Not in a folder</span>
              </Link>
            </SidebarMenuButton>
          </div>
        </SidebarMenuItem>
        {isPending &&
          Array.from({ length: 3 }, (_, i) => (
            <SidebarMenuItem key={i}>
              <SidebarMenuSkeleton showIcon />
            </SidebarMenuItem>
          ))}
        <FolderRows
          nodes={buildFolderTree(folders)}
          depth={0}
          open={open}
          isExpanded={isExpanded}
          onToggle={(id, expanded) => setToggled((was) => ({ ...was, [id]: expanded }))}
        />
      </SidebarMenu>
    </SidebarGroup>
  );
}
