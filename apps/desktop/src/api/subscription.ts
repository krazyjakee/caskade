/** Starts an event subscription from a React effect and returns the effect's
 * cleanup. Registration is asynchronous: a cleanup that runs first stops the
 * subscription as soon as it arrives, and no event reaches `listener` after
 * the cleanup. A failed registration calls `failed` with the error; views
 * still refetch on focus and poll where they need to. */
export function effectSubscription<A extends unknown[]>(
  subscribe: (listener: (...args: A) => void) => Promise<() => void>,
  listener: (...args: A) => void,
  { subscribed, failed }: { subscribed?: () => void; failed?: (error: unknown) => void } = {},
): () => void {
  let disposed = false;
  let stop: (() => void) | undefined;
  subscribe((...args) => {
    if (!disposed) listener(...args);
  })
    .then(
      (cleanup) => {
        if (disposed) return cleanup();
        stop = cleanup;
        subscribed?.();
      },
      (error: unknown) => {
        if (!disposed) failed?.(error);
      },
    )
    // A throwing callback must not surface as an unhandled rejection.
    .catch(() => {});
  return () => {
    disposed = true;
    stop?.();
  };
}
