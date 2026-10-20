import { useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';

import { ErrorState } from '@/components/custom-ui/error-state';
import { Skeleton } from '@/components/ui/skeleton';

import { ontologySessionQuery } from './api';

/** What the embedded UI says once it can take a session (onto-intelli's `onto:ready`). */
const READY = 'onto:ready';

/**
 * onto-intelli's own UI, embedded: the workspace map, an entity, the schema editor or the
 * proposals inbox. The backend mints the member's own short-lived session; it is handed to the
 * frame with `postMessage` once the frame announces itself, to the frame's origin only, and never
 * travels in an address.
 */
export function OntoFrame({
  workspaceId,
  view,
  title,
}: {
  workspaceId: string;
  /** The UI's own route, after `#`: `/map?workspace=…&focus=…`, `/entities/<id>`, `/schema`. */
  view: string;
  title: string;
}) {
  const { data: session, isPending, error, refetch } = useQuery(ontologySessionQuery(workspaceId));
  const frame = useRef<HTMLIFrameElement>(null);
  const [ready, setReady] = useState(false);
  const origin = useMemo(() => {
    if (!session) return null;
    try {
      return new URL(session.ui_url, window.location.href).origin;
    } catch {
      return null;
    }
  }, [session]);

  useEffect(() => {
    if (!session || !origin) return;
    const hand = () =>
      frame.current?.contentWindow?.postMessage(
        {
          type: 'onto:session',
          token: session.token,
          tenant: session.tenant,
          workspace: session.workspace,
        },
        origin,
      );
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== origin || event.source !== frame.current?.contentWindow) return;
      const data: unknown = event.data;
      if (
        typeof data === 'object' &&
        data !== null &&
        (data as { type?: unknown }).type === READY
      ) {
        setReady(true);
        hand();
      }
    };
    window.addEventListener('message', onMessage);
    // A renewed session goes to a frame that announced itself already.
    if (ready) hand();
    return () => window.removeEventListener('message', onMessage);
  }, [session, origin, ready]);

  if (isPending) return <Skeleton className="h-[70vh] w-full rounded-lg" />;
  if (error || !session || !origin) {
    return (
      <ErrorState
        title="Couldn’t open onto-intelli"
        error={error ?? new Error('onto-intelli’s address is not valid')}
        onRetry={() => void refetch()}
      />
    );
  }
  const src = `${session.ui_url}#${view}`;
  return (
    <iframe
      ref={frame}
      // A new view is a new document: the frame announces itself again and gets the session.
      key={src}
      src={src}
      title={title}
      className="h-[70vh] min-h-[28rem] w-full rounded-lg border bg-background"
      referrerPolicy="no-referrer"
      sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
      allow="clipboard-write"
    />
  );
}
