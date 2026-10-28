import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import { FRESH } from '@/lib/freshness';
import { qk } from '@/lib/query-keys';
import { skillSchema, skillSummarySchema } from '@/schemas/skill';

/** Every skill of the library, without their bodies: names for chips, the picker and the page. */
export const skillsQuery = () =>
  queryOptions({
    queryKey: qk.skills.list,
    queryFn: ({ signal }) => apiRequest('/skills', z.array(skillSummarySchema), { signal }),
    staleTime: FRESH.catalogue,
  });

/** One skill with its working knowledge, in Markdown. */
export const skillQuery = (skillId: string) =>
  queryOptions({
    queryKey: qk.skills.one(skillId),
    queryFn: ({ signal }) =>
      apiRequest(`/skills/${encodeURIComponent(skillId)}`, skillSchema, { signal }),
    staleTime: FRESH.catalogue,
  });
