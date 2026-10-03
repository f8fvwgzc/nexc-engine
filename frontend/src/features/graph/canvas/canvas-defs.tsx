/** Arrowhead markers for depends_on edges (default, selected, suggestion). */
export function CanvasDefs() {
  return (
    <defs>
      {(
        [
          ['garrow', 'garrow'],
          ['garrow-active', 'garrow-active'],
          ['garrow-brand', 'garrow-brand'],
        ] as const
      ).map(([id, className]) => (
        <marker
          key={id}
          id={id}
          viewBox="0 0 10 10"
          refX="8"
          refY="5"
          markerWidth="7"
          markerHeight="7"
          orient="auto-start-reverse"
        >
          <path d="M0,0 L10,5 L0,10 z" className={className} />
        </marker>
      ))}
    </defs>
  );
}
