import {
  ArrowRightLeftIcon,
  BotIcon,
  CalendarIcon,
  DownloadIcon,
  GitPullRequestIcon,
  HeartIcon,
  LinkIcon,
  MessagesSquareIcon,
  MoreHorizontalIcon,
  PaperclipIcon,
  PencilLineIcon,
  SignalHighIcon,
  SparklesIcon,
  UserRoundIcon,
  type LucideIcon,
} from 'lucide-react';
import { m } from 'motion/react';
import type { ComponentProps, ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { toast } from 'sonner';

import { IconButton } from '@/components/custom-ui/icon-button';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { downloadArtifactFile } from '@/features/artifacts/api';
import { ArtifactGlyph } from '@/features/artifacts/artifact-glyphs';
import { errorMessage } from '@/lib/api/errors';
import { copyText } from '@/lib/clipboard';
import { formatBytes, formatDateTime, formatRelative } from '@/lib/format';
import { rise } from '@/lib/motion';
import { cn } from '@/lib/utils';
import type { ArtifactFile } from '@/schemas/artifact';
import type { IssueEvent } from '@/schemas/issue';

import { changeText } from './activity';

const MUTED = 'text-muted-foreground/80';

/** The glyph, and its colour, that marks each kind of entry in the activity's first column. */
const KIND: Record<IssueEvent['kind'], { icon: LucideIcon; tone: string }> = {
  comment: {
    icon: MessagesSquareIcon,
    tone: 'fill-emerald-500/20 text-emerald-600 dark:text-emerald-400',
  },
  artifact: { icon: PaperclipIcon, tone: 'text-sky-600 dark:text-sky-400' },
  run: { icon: SparklesIcon, tone: 'fill-violet-500/20 text-violet-600 dark:text-violet-400' },
  state: { icon: ArrowRightLeftIcon, tone: MUTED },
  priority: { icon: SignalHighIcon, tone: MUTED },
  assignee: { icon: UserRoundIcon, tone: MUTED },
  title: { icon: PencilLineIcon, tone: MUTED },
  due: { icon: CalendarIcon, tone: MUTED },
  github: { icon: GitPullRequestIcon, tone: MUTED },
  agent: { icon: BotIcon, tone: MUTED },
};

export function KindIcon({ kind, className }: { kind: IssueEvent['kind']; className?: string }) {
  const { icon: Icon, tone } = KIND[kind];
  return <Icon aria-hidden className={cn('size-4', tone, className)} />;
}

/** What marks the line of who reacted to an entry. */
export function ReactionsIcon() {
  return <HeartIcon aria-hidden className="size-4 fill-rose-500 text-rose-500" />;
}

/** A person beside what they wrote or changed; `small` for a reply or a one-line change. */
export function EntryAvatar({
  name,
  small = false,
}: {
  name: string | undefined;
  small?: boolean;
}) {
  return <PersonGlyph name={name} className={small ? 'size-5 text-[9px]' : 'size-6 text-[10px]'} />;
}

/** The agents, beside what a run of theirs reported. */
export function AgentsAvatar() {
  return (
    <span className="flex size-6 items-center justify-center rounded-full bg-violet-500/15 text-violet-600 dark:text-violet-400">
      <BotIcon className="size-3.5" />
    </span>
  );
}

/** The columns every entry lines up on: what kind it is, who, and what it says. */
const COLUMNS =
  'grid grid-cols-[1.25rem_1.5rem_minmax(0,1fr)] gap-x-2.5 sm:grid-cols-[1.75rem_1.5rem_minmax(0,1fr)] sm:gap-x-3';

/** A row on the entries' columns outside the list of them (the box for a new comment). */
export function EntryGrid({ className, ...props }: ComponentProps<'div'>) {
  return <div className={cn(COLUMNS, className)} {...props} />;
}

/**
 * One entry of the activity, rising into place when it is new. A `highlighted` one (a run's
 * report, a file) sits on a soft card that reaches a little past the columns, so its text still
 * lines up with the entries around it.
 */
export function ActivityEntry({
  highlighted = false,
  className,
  ...props
}: ComponentProps<typeof m.li> & { highlighted?: boolean }) {
  return (
    <m.li
      {...rise}
      className={cn(
        COLUMNS,
        highlighted && '-mx-2 rounded-2xl bg-muted/50 px-2 py-3 sm:-mx-3 sm:px-3',
        className,
      )}
      {...props}
    />
  );
}

/**
 * One row of an entry, on the entry's columns: what marks it, whose it is, and what it says.
 * `threaded` carries the entry's line on from under the avatar to the row below it (a reply, the
 * reply box). The marks repeat the text beside them, so they are hidden from screen readers.
 */
export function EntryRow({
  icon,
  avatar,
  threaded = false,
  className,
  children,
  ...props
}: Omit<ComponentProps<typeof m.div>, 'children'> & {
  icon?: ReactNode;
  avatar?: ReactNode;
  threaded?: boolean;
  children: ReactNode;
}) {
  return (
    <m.div className="col-span-3 grid grid-cols-subgrid" {...props}>
      <span aria-hidden className="flex h-6 items-center justify-center">
        {icon}
      </span>
      <span aria-hidden className="flex flex-col items-center">
        {avatar && <span className="flex h-6 items-center">{avatar}</span>}
        {threaded && <span className={cn('w-px flex-1 bg-border', avatar && 'mt-1')} />}
      </span>
      <div className={cn('min-w-0', threaded && 'pb-3', className)}>{children}</div>
    </m.div>
  );
}

/** When something happened, relative, with the full date on hover. */
export function EntryTime({ at, edited = false }: { at: string; edited?: boolean }) {
  return (
    <time
      dateTime={at}
      title={formatDateTime(at)}
      className="shrink-0 py-0.5 text-xs leading-5 text-muted-foreground tabular-nums"
    >
      {formatRelative(at)}
      {edited ? ' · edited' : ''}
    </time>
  );
}

/**
 * "**Name** did this", what can be done to it, and when, on the right. The line is cut short
 * where it does not fit, unless it `wraps` (a change, whose end is the point).
 */
export function EntryHeader({
  name,
  action,
  at,
  edited,
  actions,
  wraps = false,
}: {
  name: string;
  action?: ReactNode;
  at: string;
  edited?: boolean;
  /** Buttons between the text and the time. */
  actions?: ReactNode;
  wraps?: boolean;
}) {
  return (
    <div className="relative flex min-h-6 items-start gap-2 text-[13px]">
      <p
        className={cn(
          'min-w-0 flex-1 py-0.5 leading-5 text-muted-foreground',
          wraps ? 'break-words' : 'truncate',
        )}
      >
        <span className="font-medium text-foreground">{name}</span>
        {action && <> {action}</>}
      </p>
      {actions}
      <EntryTime at={at} edited={edited} />
    </div>
  );
}

/** The `⋯` menu of an entry. */
export function EntryMenu({
  label,
  children,
  className,
  onCloseAutoFocus,
}: {
  label: string;
  children: ReactNode;
  className?: string;
  onCloseAutoFocus?: ComponentProps<typeof DropdownMenuContent>['onCloseAutoFocus'];
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <IconButton
          label={label}
          size="icon-xs"
          className={cn('text-muted-foreground hover:text-foreground', className)}
        >
          <MoreHorizontalIcon />
        </IconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-40" onCloseAutoFocus={onCloseAutoFocus}>
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Changes one person made one after another, as one entry of short muted lines: the person and
 * the first change, each further change under it, and the time of the last.
 */
export function ChangeEntry({
  events,
  name,
  person,
}: {
  events: IssueEvent[];
  /** Who made the changes, as the line names them. */
  name: string;
  /** Whom the avatar shows; none for someone who left. */
  person: string | undefined;
}) {
  const [first, ...rest] = events;
  if (!first) return null;
  return (
    <ActivityEntry data-compact>
      <EntryRow icon={<KindIcon kind={first.kind} />} avatar={<EntryAvatar name={person} small />}>
        <EntryHeader
          name={name}
          action={changeText(first)}
          at={(events.at(-1) ?? first).created_at}
          wraps
        />
        {rest.length > 0 && (
          <ul className="space-y-1 pb-0.5 text-[13px] text-muted-foreground">
            {rest.map((event) => (
              <li key={event.id} className="flex min-w-0 items-start gap-1.5">
                <KindIcon kind={event.kind} className="mt-0.5 size-3.5 shrink-0" />
                <span className="min-w-0 break-words">{changeText(event)}</span>
              </li>
            ))}
          </ul>
        )}
      </EntryRow>
    </ActivityEntry>
  );
}

/**
 * A file of the issue that was added or changed, on a card: the file, and the buttons that open
 * and download it. `file` is the issue's file of that name, if it still has one.
 */
export function ArtifactEntry({
  event,
  name,
  file,
}: {
  event: IssueEvent;
  name: string;
  file: ArtifactFile | undefined;
}) {
  const fileName = event.body || 'a file';
  const download = (target: ArtifactFile) => {
    downloadArtifactFile(target).catch((error: unknown) => toast.error(errorMessage(error)));
  };
  return (
    <ActivityEntry highlighted>
      <EntryRow icon={<KindIcon kind="artifact" />} avatar={<EntryAvatar name={name} />}>
        <EntryHeader name={name} action={changeText(event)} at={event.created_at} />
        <div className="mt-2 flex items-center gap-3 rounded-xl bg-background/80 p-3 dark:bg-background/40">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-sky-500/10 text-sky-600 dark:text-sky-400">
            <ArtifactGlyph kind={file?.kind ?? 'text'} className="size-4 text-current" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium" title={fileName}>
              {fileName}
            </span>
            <span className="block text-xs text-muted-foreground">
              {file
                ? `v${file.current_version} · ${formatBytes(file.size)}`
                : 'No longer on this issue'}
            </span>
          </span>
        </div>
        {file && (
          <div className="mt-3 flex items-center gap-2">
            <Button size="sm" className="rounded-full px-3.5" asChild>
              <Link to={`/app/artifacts/${file.id}`} aria-label={`Open ${file.name}`}>
                Open
              </Link>
            </Button>
            <Button
              size="sm"
              variant="secondary"
              className="rounded-full px-3.5"
              aria-label={`Download ${file.name}`}
              onClick={() => download(file)}
            >
              <DownloadIcon />
              Download
            </Button>
            <EntryMenu label={`More for ${file.name}`} className="ml-auto rounded-full">
              <DropdownMenuItem
                onSelect={() =>
                  void copyText(
                    `${window.location.origin}/app/artifacts/${file.id}`,
                    `Copied the link to ${file.name}`,
                  )
                }
              >
                <LinkIcon />
                Copy link
              </DropdownMenuItem>
            </EntryMenu>
          </div>
        )}
      </EntryRow>
    </ActivityEntry>
  );
}
