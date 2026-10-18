import { CheckIcon, CopyIcon } from 'lucide-react';
import { useEffect, useState } from 'react';
import { toast } from 'sonner';

import { cn } from '@/lib/utils';

import { IconButton } from './icon-button';

export function CopyButton({
  value,
  label = 'Copy',
  className,
}: {
  value: string;
  label?: string;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
    } catch {
      toast.error('Clipboard is not available');
    }
  };

  return (
    <IconButton
      type="button"
      label={copied ? 'Copied' : label}
      onClick={() => void copy()}
      className={cn('text-muted-foreground hover:text-foreground', className)}
    >
      {copied ? (
        <CheckIcon className="text-status-succeeded motion-safe:animate-scale-in" />
      ) : (
        <CopyIcon />
      )}
    </IconButton>
  );
}
