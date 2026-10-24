import { FileCodeIcon, FileIcon, FileTextIcon, GlobeIcon, type LucideIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { cn } from '@/lib/utils';
import type { ArtifactFile, ArtifactKind } from '@/schemas/artifact';

const KIND_ICON: Record<ArtifactKind, LucideIcon> = {
  markdown: FileTextIcon,
  html: GlobeIcon,
  code: FileCodeIcon,
  text: FileIcon,
};

/** A file's glyph by how it previews: Markdown, a page, code or plain text. */
export function ArtifactGlyph({ kind, className }: { kind: ArtifactKind; className?: string }) {
  const Icon = KIND_ICON[kind];
  return <Icon className={cn('size-3.5 shrink-0 text-muted-foreground', className)} aria-hidden />;
}

/** Where an issue opens. */
const issuePath = (issueId: string) => `/app/issues/${issueId}`;

/** The identifier of the issue a file belongs to, as a link to the issue. */
export function IssueChip({ issue }: { issue: NonNullable<ArtifactFile['issue']> }) {
  return (
    <Link
      to={issuePath(issue.id)}
      title={issue.title}
      onClick={(e) => e.stopPropagation()}
      className="inline-flex h-5 max-w-full items-center rounded border px-1.5 font-mono text-[11px] text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
    >
      <span className="truncate">{issue.identifier}</span>
    </Link>
  );
}
