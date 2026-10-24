import { useCallback, useState } from 'react';

const foldedKey = (teamId: string) => `nexc.board.folded.${teamId}`;

/** The columns (state ids) folded on a team's board; none when nothing is remembered. */
export function readFoldedColumns(teamId: string): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(foldedKey(teamId)) ?? '[]');
    return Array.isArray(raw) ? raw.filter((id): id is string => typeof id === 'string') : [];
  } catch {
    return [];
  }
}

export function writeFoldedColumns(teamId: string, stateIds: string[]) {
  try {
    if (stateIds.length === 0) localStorage.removeItem(foldedKey(teamId));
    else localStorage.setItem(foldedKey(teamId), JSON.stringify(stateIds));
  } catch {
    // Folded for this visit only.
  }
}

/**
 * The folded columns of a team's board, and a toggle that remembers them for that team. The
 * board is keyed by its team, so another team starts from its own remembered columns.
 */
export function useFoldedColumns(teamId: string): [string[], (stateId: string) => void] {
  const [folded, setFolded] = useState(() => readFoldedColumns(teamId));
  const toggle = useCallback(
    (stateId: string) =>
      setFolded((current) => {
        const next = current.includes(stateId)
          ? current.filter((id) => id !== stateId)
          : [...current, stateId];
        writeFoldedColumns(teamId, next);
        return next;
      }),
    [teamId],
  );
  return [folded, toggle];
}
