// Whether movement should be kept to a minimum: the Reduce motion setting
// (a class on the app root) or the system's reduced-motion preference.
export function reducedMotion(): boolean {
  if (document.querySelector('.app.reduce-motion')) return true;
  return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
}
