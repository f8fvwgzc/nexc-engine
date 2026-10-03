import { useId, type ReactNode } from 'react';
import {
  Controller,
  type Control,
  type ControllerRenderProps,
  type FieldPath,
  type FieldValues,
} from 'react-hook-form';

import { Field, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field';

export type FormControlProps<T extends FieldValues, N extends FieldPath<T>> = ControllerRenderProps<
  T,
  N
> & {
  id: string;
  'aria-invalid': boolean;
  'aria-describedby': string | undefined;
};

interface FormFieldProps<T extends FieldValues, N extends FieldPath<T>> {
  control: Control<T>;
  name: N;
  label: ReactNode;
  description?: ReactNode;
  className?: string;
  children: (field: FormControlProps<T, N>) => ReactNode;
}

/**
 * react-hook-form Controller + shadcn Field (label, description, error) with the accessibility
 * wiring (`aria-invalid`, `aria-describedby`) done once.
 */
export function FormField<T extends FieldValues, N extends FieldPath<T>>({
  control,
  name,
  label,
  description,
  className,
  children,
}: FormFieldProps<T, N>) {
  const id = useId();
  const descriptionId = `${id}-description`;
  const errorId = `${id}-error`;
  return (
    <Controller
      control={control}
      name={name}
      render={({ field, fieldState }) => {
        const describedBy =
          [description ? descriptionId : null, fieldState.error ? errorId : null]
            .filter(Boolean)
            .join(' ') || undefined;
        return (
          <Field data-invalid={fieldState.invalid} className={className}>
            <FieldLabel htmlFor={id}>{label}</FieldLabel>
            {children({
              ...field,
              id,
              'aria-invalid': fieldState.invalid,
              'aria-describedby': describedBy,
            })}
            {description && <FieldDescription id={descriptionId}>{description}</FieldDescription>}
            <FieldError id={errorId} errors={[fieldState.error]} />
          </Field>
        );
      }}
    />
  );
}
