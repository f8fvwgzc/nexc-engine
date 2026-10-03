import type { SVGProps } from 'react';

import type { NodeKind } from '@/schemas/graph';

import { NODE_KIND_META } from './node-kind-meta';

type NodeKindIconProps = SVGProps<SVGSVGElement> & { kind: NodeKind; size?: number };

export function NodeKindIcon({ kind, size = 16, ...props }: NodeKindIconProps) {
  const Icon = NODE_KIND_META[kind].icon;
  return <Icon width={size} height={size} aria-label={NODE_KIND_META[kind].label} {...props} />;
}
