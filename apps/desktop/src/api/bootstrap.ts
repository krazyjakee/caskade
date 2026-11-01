import { createNativeApi } from "./native";
import type { LibraryApi } from "./types";
export async function connectLibraryApi(): Promise<LibraryApi> {
  // Vite eliminates this import and fixture bytes from production builds.
  if (import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1") {
    const { createEmulatedFixtureGames, createFixtureApi, createFixtureGames } = await import(
      "./fixtures"
    );
    const requested = Number(import.meta.env.VITE_CASKADE_FIXTURE_COUNT ?? 24);
    const count = Number.isInteger(requested) ? Math.min(10_000, Math.max(1, requested)) : 24;
    return createFixtureApi([...createFixtureGames(count), ...createEmulatedFixtureGames()]);
  }
  return createNativeApi();
}
