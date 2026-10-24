import { useMutation, useQuery } from '@tanstack/react-query';
import { useState, type ReactNode } from 'react';

import { OptionSelect } from '@/components/custom-ui/option-select';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { errorMessage } from '@/lib/api/errors';

import { artifactFoldersQuery } from './api';
import { buildFolderTree, flattenTree } from './files';

interface NameDialogProps {
  onClose: () => void;
  title: string;
  description?: ReactNode;
  /** The field's accessible name. */
  label: string;
  initial?: string;
  placeholder?: string;
  submitLabel: string;
  /** Selects the name without its extension when the field gets focus (a file's name). */
  selectStem?: boolean;
  /** Why a name cannot be used, or null. */
  check: (name: string) => string | null;
  onSubmit: (name: string) => Promise<unknown>;
}

/**
 * Asks for a name: a new folder or file, or a new name for one. What is wrong with the name shows
 * under the field as it is typed, and so does what the server said (a name taken, say) until the
 * name is changed. The dialog stays open on a refusal, with nothing made.
 */
export function NameDialog({
  onClose,
  title,
  description,
  label,
  initial = '',
  placeholder,
  submitLabel,
  selectStem = false,
  check,
  onSubmit,
}: NameDialogProps) {
  const [name, setName] = useState(initial);
  const save = useMutation({
    mutationFn: () => onSubmit(name.trim()),
    meta: { errorToast: false },
    onSuccess: onClose,
  });
  const trimmed = name.trim();
  const problem = check(trimmed);
  const unchanged = initial !== '' && trimmed === initial;
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (!problem && !unchanged) save.mutate();
          }}
        >
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            {description && <DialogDescription>{description}</DialogDescription>}
          </DialogHeader>
          <Input
            autoFocus
            value={name}
            placeholder={placeholder}
            aria-label={label}
            aria-invalid={trimmed !== '' && problem !== null}
            onChange={(e) => {
              setName(e.target.value);
              // What the server said was about the name it was sent, not this one.
              if (save.isError) save.reset();
            }}
            onFocus={(e) => {
              if (!selectStem) return;
              // The name without its extension, as file managers select it.
              const dot = e.target.value.lastIndexOf('.');
              e.target.setSelectionRange(0, dot > 0 ? dot : e.target.value.length);
            }}
          />
          {trimmed !== '' && problem && (
            <p role="alert" className="text-sm text-destructive">
              {problem}
            </p>
          )}
          {save.error && (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(save.error)}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={problem !== null || unchanged || save.isPending}>
              {submitLabel}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

const TOP = 'top';

interface MoveDialogProps {
  workspaceId: string;
  onClose: () => void;
  title: string;
  /** The folder it is in now; null at the top level. */
  current: string | null;
  /** What the top level is called: "Not in a folder" for a file, "Top level" for a folder. */
  topLabel: string;
  /** Folders it cannot go into: a folder itself and everything under it. */
  exclude?: Set<string>;
  onMove: (folderId: string | null) => Promise<unknown>;
}

/** Picks the folder to move a file or a folder into, from the whole tree. */
export function MoveDialog({
  workspaceId,
  onClose,
  title,
  current,
  topLabel,
  exclude,
  onMove,
}: MoveDialogProps) {
  const { data: folders = [] } = useQuery(artifactFoldersQuery(workspaceId));
  const [target, setTarget] = useState(current ?? TOP);
  const move = useMutation({
    mutationFn: () => onMove(target === TOP ? null : target),
    meta: { errorToast: false },
    onSuccess: onClose,
  });
  const options = [
    { value: TOP, label: topLabel },
    ...flattenTree(buildFolderTree(folders))
      .filter(({ folder }) => !exclude?.has(folder.id))
      .map(({ folder, depth }) => ({
        value: folder.id,
        label: (
          <span className="truncate" style={{ paddingLeft: `${depth * 0.75}rem` }}>
            {folder.name}
          </span>
        ),
      })),
  ];
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            move.mutate();
          }}
        >
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription>Choose the folder it goes into.</DialogDescription>
          </DialogHeader>
          <OptionSelect
            value={target}
            onValueChange={setTarget}
            options={options}
            aria-label="Folder"
          />
          {move.error && (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(move.error)}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={(target === TOP ? null : target) === current || move.isPending}
            >
              Move
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
