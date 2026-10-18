import type { ComponentProps, ReactNode } from 'react';

import { Button } from '@/components/ui/button';
import type { TooltipContent } from '@/components/ui/tooltip';

import { WithTooltip } from './with-tooltip';

interface IconButtonProps extends Omit<ComponentProps<typeof Button>, 'aria-label' | 'children'> {
  /** What the button does: its accessible name and the tooltip shown on hover and focus. */
  label: string;
  /** The icon (or, with `asChild`, the link or element that holds it). */
  children: ReactNode;
  tooltipSide?: ComponentProps<typeof TooltipContent>['side'];
}

/**
 * A button that shows only an icon. The label is required: it names the button for assistive
 * technology and appears as a tooltip, so a sighted reader can tell what the icon does. Works
 * inside menu triggers (`DropdownMenuTrigger asChild`) and with `asChild` for links.
 */
export function IconButton({
  label,
  children,
  tooltipSide,
  variant = 'ghost',
  size = 'icon-sm',
  ...props
}: IconButtonProps) {
  return (
    <WithTooltip label={label} side={tooltipSide}>
      <Button variant={variant} size={size} aria-label={label} {...props}>
        {children}
      </Button>
    </WithTooltip>
  );
}
