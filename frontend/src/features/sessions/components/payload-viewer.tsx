import { ChevronRightIcon, EyeOffIcon } from 'lucide-react';
import { useId, useState } from 'react';

import { CopyButton } from '@/components/custom-ui/copy-button';
import { IconButton } from '@/components/custom-ui/icon-button';
import { Badge } from '@/components/ui/badge';
import { formatBytes } from '@/lib/format';
import { cn } from '@/lib/utils';
import { PAYLOAD_KIND_LABEL, type SpanPayload } from '@/schemas/session';

import { countMarkers, splitMarkers } from '../redaction';

/** Payloads longer than this open folded. */
const FOLD_OVER_BYTES = 4 * 1024;

/**
 * One recorded payload: what a turn was given or answered, what a tool got or gave. Monospace,
 * collapsible, copied with a click; what the recording replaced (a redacted passage, a scrubbed
 * credential, a cut) stands out as a marker.
 */
export function PayloadViewer({ payload }: { payload: SpanPayload }) {
  const [open, setOpen] = useState(payload.size <= FOLD_OVER_BYTES);
  const bodyId = useId();
  const markers = countMarkers(payload.content);
  const pieces = splitMarkers(payload.content);
  return (
    <section
      className="rounded-lg border bg-muted/20"
      aria-label={PAYLOAD_KIND_LABEL[payload.kind]}
      data-kind={payload.kind}
    >
      <header className="flex flex-wrap items-center gap-2 px-2 py-1.5 text-xs">
        <IconButton
          type="button"
          size="icon-xs"
          aria-expanded={open}
          aria-controls={bodyId}
          label={open ? 'Fold the payload' : 'Unfold the payload'}
          onClick={() => setOpen((o) => !o)}
        >
          <ChevronRightIcon className={cn('transition-transform', open && 'rotate-90')} />
        </IconButton>
        <span className="font-medium">{PAYLOAD_KIND_LABEL[payload.kind]}</span>
        <span className="text-muted-foreground tabular-nums">{formatBytes(payload.size)}</span>
        {(payload.redactions > 0 || markers > 0) && (
          <Badge variant="outline" className="gap-1 text-[10px]">
            <EyeOffIcon aria-hidden />
            {payload.redactions > 0 ? payload.redactions : markers} redacted
          </Badge>
        )}
        <CopyButton
          value={payload.content}
          label={`Copy the ${PAYLOAD_KIND_LABEL[payload.kind].toLowerCase()}`}
          className="ml-auto"
        />
      </header>
      <pre
        id={bodyId}
        hidden={!open}
        className="max-h-[32rem] overflow-auto border-t px-3 py-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap"
      >
        {pieces.map((piece, i) =>
          piece.marker ? (
            <mark
              key={i}
              className="rounded-sm bg-[color-mix(in_oklch,var(--status-queued)_25%,transparent)] px-1 font-medium text-foreground"
              data-marker
            >
              {piece.text}
            </mark>
          ) : (
            <span key={i}>{piece.text}</span>
          ),
        )}
      </pre>
    </section>
  );
}
