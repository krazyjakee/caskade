/** An API object whose methods load their implementation on first call, so
 * a synchronously exported backend can be a development fixture that a
 * production build drops (the import sits behind an `import.meta.env.DEV`
 * check at the call site; see README.md). */
export function lazyApi<T extends object>(
  load: () => Promise<T>,
  methods: readonly (keyof T & string)[],
): T {
  let loading: Promise<T> | undefined;
  const api: Record<string, unknown> = {};
  for (const method of methods) {
    api[method] = async (...args: unknown[]) => {
      loading ??= load();
      const target = await loading;
      const call = target[method] as unknown as (...args: unknown[]) => unknown;
      return call.apply(target, args);
    };
  }
  return api as T;
}
