import { useState } from 'react';

/** The teams folded in the sidebar, by id. A team starts unfolded, so new teams show their rows. */
export const FOLDED_TEAMS_KEY = 'nexc.sidebar.folded-teams';

/** The folded teams this browser remembers; none when storage is unavailable or garbled. */
export function readFoldedTeams(): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(FOLDED_TEAMS_KEY) ?? '[]');
    return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === 'string') : [];
  } catch {
    return [];
  }
}

/** Remembers the folded teams (best effort: storage may be unavailable). */
export function writeFoldedTeams(ids: readonly string[]): void {
  try {
    localStorage.setItem(FOLDED_TEAMS_KEY, JSON.stringify(ids));
  } catch {
    // Folded for this page lifetime only.
  }
}

/** The folded teams once `teamId` is unfolded (`open`) or folded. */
export function withTeamOpen(folded: readonly string[], teamId: string, open: boolean): string[] {
  const rest = folded.filter((id) => id !== teamId);
  return open ? rest : [...rest, teamId];
}

/** Which of the sidebar's teams are unfolded, remembered per browser. */
export function useTeamExpansion(): [
  isOpen: (teamId: string) => boolean,
  setOpen: (teamId: string, open: boolean) => void,
] {
  const [folded, setFolded] = useState(readFoldedTeams);
  const setOpen = (teamId: string, open: boolean) => {
    const next = withTeamOpen(folded, teamId, open);
    setFolded(next);
    writeFoldedTeams(next);
  };
  return [(teamId) => !folded.includes(teamId), setOpen];
}
