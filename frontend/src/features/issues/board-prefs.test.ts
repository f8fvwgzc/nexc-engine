import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { readFoldedColumns, useFoldedColumns, writeFoldedColumns } from './board-prefs';

const ENG = '59a5a0c7-c397-74d9-8335-9438fe114555';
const DES = '749ce328-6f34-7c68-8572-e2edf21c2ad5';

beforeEach(() => localStorage.clear());
afterEach(() => vi.restoreAllMocks());

describe('folded board columns', () => {
  it('are remembered per team', () => {
    writeFoldedColumns(ENG, ['done', 'canceled']);
    expect(localStorage.getItem(`nexc.board.folded.${ENG}`)).toBe('["done","canceled"]');
    expect(readFoldedColumns(ENG)).toEqual(['done', 'canceled']);
    expect(readFoldedColumns(DES)).toEqual([]);
  });

  it('leave nothing behind once every column is open again', () => {
    writeFoldedColumns(ENG, ['done']);
    writeFoldedColumns(ENG, []);
    expect(localStorage.getItem(`nexc.board.folded.${ENG}`)).toBeNull();
  });

  it('ignore what they cannot read', () => {
    localStorage.setItem(`nexc.board.folded.${ENG}`, '{"done":true}');
    expect(readFoldedColumns(ENG)).toEqual([]);
    localStorage.setItem(`nexc.board.folded.${ENG}`, '["done",4,null]');
    expect(readFoldedColumns(ENG)).toEqual(['done']);
    localStorage.setItem(`nexc.board.folded.${ENG}`, 'not json');
    expect(readFoldedColumns(ENG)).toEqual([]);
  });

  it('fold and unfold without storage', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    const { result } = renderHook(() => useFoldedColumns(ENG));
    expect(result.current[0]).toEqual([]);
    act(() => result.current[1]('done'));
    expect(result.current[0]).toEqual(['done']);
    act(() => result.current[1]('done'));
    expect(result.current[0]).toEqual([]);
  });

  it('toggle a column and keep the choice', () => {
    const { result } = renderHook(() => useFoldedColumns(ENG));
    act(() => result.current[1]('backlog'));
    expect(result.current[0]).toEqual(['backlog']);
    expect(readFoldedColumns(ENG)).toEqual(['backlog']);
  });
});
