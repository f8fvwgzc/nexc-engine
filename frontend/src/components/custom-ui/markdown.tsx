import type { ComponentProps } from 'react';
import ReactMarkdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';

import { cn } from '@/lib/utils';

/**
 * Renders model/agent output as GitHub-flavoured markdown, safely: raw HTML is never rendered
 * (react-markdown's default), images are dropped (no remote fetches from untrusted output), and
 * links open in a new tab without an opener.
 */
const components: Components = {
  a: ({ node: _node, ...props }: ComponentProps<'a'> & { node?: unknown }) => (
    <a {...props} target="_blank" rel="noopener noreferrer nofollow" />
  ),
};

export function Markdown({ children, className }: { children: string; className?: string }) {
  return (
    <div
      className={cn(
        'prose prose-sm max-w-none break-words dark:prose-invert prose-headings:font-semibold prose-headings:tracking-tight prose-a:text-brand prose-code:before:content-none prose-code:after:content-none prose-pre:bg-muted prose-pre:text-foreground',
        className,
      )}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={components}
        disallowedElements={['img']}
        unwrapDisallowed
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
