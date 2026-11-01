/** Games' videos: IGDB's YouTube trailers, on a library game's page and a
 * Store game's. They play in the video window, a window of Caskade's own
 * (YouTube's player won't play inside the app's pages); without the core,
 * the browser preview opens them on YouTube. */
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext } from "react";
import { commands, type IgdbVideo_Serialize } from "./bindings";
import { unwrap } from "./reply";

export type IgdbVideo = IgdbVideo_Serialize;

export interface VideoApi {
  /** Plays `videoId` in the video window, titled `title`; another video
   * playing stops. */
  play(videoId: string, title: string): Promise<void>;
}

/** A video's thumbnail: YouTube's 16:9 one, 320×180. */
export const videoThumbnail = (videoId: string) =>
  `https://i.ytimg.com/vi/${encodeURIComponent(videoId)}/mqdefault.jpg`;

/** The video's page on YouTube. */
export const videoPage = (videoId: string) =>
  `https://www.youtube.com/watch?v=${encodeURIComponent(videoId)}`;

export const nativeVideoApi: VideoApi = {
  play: async (videoId, title) => {
    if (!isTauri()) {
      window.open(videoPage(videoId), "_blank", "noopener,noreferrer");
      return;
    }
    await unwrap(commands.videoPlay(videoId, title));
  },
};

const VideoApiContext = createContext<VideoApi>(nativeVideoApi);

/** Tests supply their own API; the app uses the native one. */
export function VideoApiProvider({ api, children }: { api: VideoApi; children: ReactNode }) {
  return createElement(VideoApiContext.Provider, { value: api }, children);
}

export const useVideoApi = () => useContext(VideoApiContext);
