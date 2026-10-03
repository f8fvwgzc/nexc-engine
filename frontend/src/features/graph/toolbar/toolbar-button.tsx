import type { LucideIcon } from 'lucide-react';
import type { ComponentProps } from 'react';

import { KbdHint } from '@/components/custom-ui/kbd-hint';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

interface ToolbarButtonProps extends ComponentProps<typeof Button> {
  icon: LucideIcon;
  label: string;
  shortcut?: string;
  /** Show the text label next to the icon on wider screens. */
  showLabel?: boolean;
}

export function ToolbarButton({
  icon: Icon,
  label,
  shortcut,
  showLabel = false,
  ...props
}: ToolbarButtonProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size={showLabel ? 'sm' : 'icon-sm'}
          aria-label={label}
          aria-keyshortcuts={shortcut}
          {...props}
        >
          <Icon />
          {showLabel && <span className="hidden lg:inline">{label}</span>}
        </Button>
      </TooltipTrigger>
      <TooltipContent className="flex items-center gap-2">
        {label}
        {shortcut && <KbdHint keys={shortcut} />}
      </TooltipContent>
    </Tooltip>
  );
}
