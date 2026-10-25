import { useQuery } from '@tanstack/react-query';
import { LibraryIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { formatTokens } from '@/lib/format';

import { skillsQuery } from '../api';
import { categoryLabel, skillTokens } from '../categories';
import { SkillDetail } from './skill-detail';

interface SkillDialogProps {
  /** The skill shown; null closes the dialog. */
  skillId: string | null;
  onOpenChange: (open: boolean) => void;
}

/** One skill over the page that names it (an agent's card), with a way to the library. */
export function SkillDialog({ skillId, onOpenChange }: SkillDialogProps) {
  const { data: skills } = useQuery(skillsQuery());
  const summary = skills?.find((s) => s.id === skillId);
  return (
    <Dialog open={skillId !== null} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
        {skillId && (
          <>
            <DialogHeader>
              <DialogTitle>{summary?.name ?? skillId}</DialogTitle>
              <DialogDescription>
                {summary
                  ? `${categoryLabel(summary.category)} · about ${formatTokens(skillTokens(summary.chars))} tokens in a prompt`
                  : skillId}
              </DialogDescription>
            </DialogHeader>
            <SkillDetail skillId={skillId} summary={summary} />
            <DialogFooter>
              <Button variant="outline" asChild>
                <Link to={`/app/skills?skill=${encodeURIComponent(skillId)}`}>
                  <LibraryIcon />
                  Open in the skill library
                </Link>
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
