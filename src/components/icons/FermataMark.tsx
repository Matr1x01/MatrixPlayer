/** The Fermata logo: a fermata arc over its dot, in the accent colour. */
export function FermataMark({ size = 20 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="none" aria-hidden="true">
      <path d="M4 16a8 8 0 0 1 16 0" stroke="var(--ac)" strokeWidth="2.2" strokeLinecap="round" />
      <circle cx="12" cy="16" r="2.2" fill="var(--ac)" />
    </svg>
  );
}
