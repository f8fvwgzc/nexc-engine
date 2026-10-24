import { lazy, Suspense } from 'react';

import { Markdown } from '@/components/custom-ui/markdown';
import { CodeBlock, CodeBlockContent, CodeBlockLine } from '@/components/ui/code-block';
import type { ArtifactFile } from '@/schemas/artifact';

const HighlightedSource = lazy(() => import('./highlighted-source'));

/** Past this many characters the source shows plain: colouring a very large file stalls the tab. */
const HIGHLIGHT_LIMIT = 200_000;

function PlainLines({ content }: { content: string }) {
  const lines = (content.endsWith('\n') ? content.slice(0, -1) : content).split('\n');
  return lines.map((line, i) => <CodeBlockLine key={i}>{line}</CodeBlockLine>);
}

/** A file's source with line numbers, coloured when shiki knows its language. */
export function ArtifactSource({ content, language }: { content: string; language: string }) {
  const plain = content.length > HIGHLIGHT_LIMIT || language === 'plaintext';
  return (
    <CodeBlock className="text-[13px]">
      <CodeBlockContent lineNumbers>
        {plain ? (
          <PlainLines content={content} />
        ) : (
          <Suspense fallback={<PlainLines content={content} />}>
            <HighlightedSource content={content} language={language} />
          </Suspense>
        )}
      </CodeBlockContent>
    </CodeBlock>
  );
}

/**
 * A file as it reads: Markdown rendered without raw HTML or images, and an HTML page in a frame
 * sandboxed from everything (no scripts, no forms, not this origin). Other files show their
 * source.
 */
export function ArtifactPreview({
  file,
  content,
}: {
  file: Pick<ArtifactFile, 'kind' | 'language' | 'name'>;
  content: string;
}) {
  if (file.kind === 'markdown') {
    return (
      <div className="rounded-lg border px-5 py-4 sm:px-8 sm:py-6">
        <Markdown className="max-w-[52rem] sm:prose-base">{content}</Markdown>
      </div>
    );
  }
  if (file.kind === 'html') {
    return (
      <iframe
        title={`Preview of ${file.name}`}
        sandbox=""
        srcDoc={content}
        referrerPolicy="no-referrer"
        className="block h-[calc(100svh-14rem)] min-h-96 w-full rounded-lg border bg-white"
      />
    );
  }
  return <ArtifactSource content={content} language={file.language} />;
}
