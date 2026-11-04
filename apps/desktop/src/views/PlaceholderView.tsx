import { Trans, useLingui } from "@lingui/react/macro";

export function PlaceholderView({ title }: { title: string }) {
  return (
    <div className="grid h-full place-items-center">
      <div className="text-center">
        <h1 className="text-2xl font-semibold">{title}</h1>
        <p className="mt-2 text-fg-subtle">
          <Trans>Not built yet.</Trans>
        </p>
      </div>
    </div>
  );
}

/** What an address the app has no page for shows. */
export function NotFoundView() {
  const { t } = useLingui();
  return <PlaceholderView title={t`Not found`} />;
}
