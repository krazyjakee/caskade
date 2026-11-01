import { convertFileSrc } from "@tauri-apps/api/core";

/** A cached image's URL on the app's asset scheme. */
export const assetUrl = (path: string) => convertFileSrc(path, "caskade-asset");

/** Select exact cached thumbnails; the library never falls back to an original. */
export function thumbnailPath(
  artwork: { thumbnails: { size: number; path: string }[] } | null | undefined,
  size: 96 | 320 | 960,
): string | null {
  const path = artwork?.thumbnails.find((thumbnail) => thumbnail.size === size)?.path;
  if (
    !path ||
    !/^[a-f0-9]{64}\/thumb-(96|320|960)\.webp$/.test(path) ||
    !path.endsWith(`/thumb-${size}.webp`)
  )
    return null;
  return path;
}

/** The cached full-size image, for views that fill the screen with it. */
export function originalPath(artwork: { cachedPath?: string } | null | undefined): string | null {
  const path = artwork?.cachedPath;
  return path && /^[a-f0-9]{64}\/original\.webp$/.test(path) ? path : null;
}
