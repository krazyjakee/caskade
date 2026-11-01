export function PlaceholderView({ title }: { title: string }) {
  return (
    <div className="grid h-full place-items-center">
      <div className="text-center">
        <h1 className="text-2xl font-semibold">{title}</h1>
        <p className="mt-2 text-fg-subtle">Not built yet.</p>
      </div>
    </div>
  );
}
