/** The animated three-bar "now playing" mark; frozen while paused. */
export function EqIndicator({ paused = false }: { paused?: boolean }) {
  return (
    <span className={paused ? 'eqi paused' : 'eqi'} aria-hidden="true">
      <i />
      <i />
      <i />
    </span>
  );
}
