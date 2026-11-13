import { Trans } from "@lingui/react/macro";

/** What an address the app has no page for shows. */
export function NotFoundView() {
  return (
    <div className="grid h-full place-items-center">
      <div className="text-center">
        <h1 className="text-2xl font-semibold">
          <Trans>Not found</Trans>
        </h1>
        <p className="mt-2 text-fg-subtle">
          <Trans>There's no page at this address.</Trans>
        </p>
      </div>
    </div>
  );
}
