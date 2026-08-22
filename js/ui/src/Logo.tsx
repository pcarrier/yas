/**
 * The YAS mark, naked.
 *
 * The same geometry as the repository's `logo.svg` and the website's: a ring
 * and three spokes, on nothing. No plate, no inverted white-on-black variant —
 * one mark everywhere it appears.
 *
 * Drawn in `currentColor` so it takes the colour of whatever it sits in rather
 * than carrying a background to stay legible. That is what lets it stay naked
 * on a dark chrome as well as a light one.
 */
export function YasMark(props: { size: number; title?: string }) {
  return (
    <svg
      width={props.size}
      height={props.size}
      viewBox="0 0 256 256"
      role={props.title ? "img" : "presentation"}
      aria-label={props.title}
      aria-hidden={props.title ? undefined : "true"}
    >
      <circle
        cx="128"
        cy="128"
        r="120"
        fill="none"
        stroke="currentColor"
        stroke-width="16"
      />
      <g fill="currentColor">
        <rect x="120" y="128" width="16" height="120" />
        <rect
          x="120"
          y="128"
          width="16"
          height="120"
          transform="rotate(120 128 128)"
        />
        <rect
          x="120"
          y="128"
          width="16"
          height="120"
          transform="rotate(240 128 128)"
        />
      </g>
    </svg>
  );
}
