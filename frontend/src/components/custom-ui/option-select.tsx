import type { ReactNode } from 'react';

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';

export interface Option<V extends string> {
  value: V;
  label: ReactNode;
}

interface OptionSelectProps<V extends string> {
  value: V;
  onValueChange: (value: V) => void;
  options: readonly Option<V>[];
  id?: string;
  placeholder?: string;
  className?: string;
  disabled?: boolean;
  'aria-invalid'?: boolean;
  'aria-describedby'?: string;
  'aria-label'?: string;
}

/** Typed wrapper around the shadcn Select for a fixed list of string options. */
export function OptionSelect<V extends string>({
  value,
  onValueChange,
  options,
  id,
  placeholder,
  className,
  disabled,
  ...aria
}: OptionSelectProps<V>) {
  return (
    <Select
      value={value}
      disabled={disabled}
      onValueChange={(v) => {
        const match = options.find((o) => o.value === v);
        if (match) onValueChange(match.value);
      }}
    >
      <SelectTrigger id={id} className={cn('w-full', className)} {...aria}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
