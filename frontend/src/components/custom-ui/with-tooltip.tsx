import type { ComponentProps, ReactElement, ReactNode } from 'react';

import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';

interface WithTooltipProps {
  /** What the control does, shown on hover and on keyboard focus. */
  label: ReactNode;
  /** One focusable element: a button, a switch, a link. */
  children: ReactElement;
  side?: ComponentProps<typeof TooltipContent>['side'];
}

/**
 * Names a control that shows no text of its own (an icon, a switch) with a tooltip. It brings its
 * own provider with the app's delay, so it also works where none is mounted (tests, portals).
 */
export function WithTooltip({ label, children, side }: WithTooltipProps) {
  return (
    <TooltipProvider delayDuration={250}>
      <Tooltip>
        <TooltipTrigger asChild>{children}</TooltipTrigger>
        <TooltipContent side={side}>{label}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
