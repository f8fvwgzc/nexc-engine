import type { SVGProps } from 'react';

import { useNodeType } from './node-kind-meta';

type NodeKindIconProps = SVGProps<SVGSVGElement> & { kind: string; size?: number };

/** Icon of a node kind, as the current graph's ontology defines it. */
export function NodeKindIcon({ kind, size = 16, style, ...props }: NodeKindIconProps) {
  const { icon: Icon, label, color } = useNodeType(kind);
  return (
    <Icon
      width={size}
      height={size}
      aria-label={label}
      style={color ? { color, ...style } : style}
      {...props}
    />
  );
}
