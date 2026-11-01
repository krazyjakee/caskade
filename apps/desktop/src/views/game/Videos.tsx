import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { Play } from "lucide-react";
import { useId } from "react";
import { Button } from "react-aria-components";
import { useGameIgdb } from "../../api/igdb";
import { type IgdbVideo, useVideoApi, videoThumbnail } from "../../api/videos";

/** A library game's videos: its IGDB match's, from the cache. */
export function GameVideos({ gameId, title }: { gameId: string; title: string }) {
  const igdb = useGameIgdb(gameId);
  return <Videos title={title} videos={igdb.data?.game?.videos ?? []} />;
}

/** A game's videos (IGDB's trailers) in a row; each plays in the video
 * window. Nothing when the game has none. */
export function Videos({ title, videos }: { title: string; videos: IgdbVideo[] }) {
  const { t } = useLingui();
  const api = useVideoApi();
  const heading = useId();
  const play = useMutation({
    mutationFn: ({ videoId, name }: { videoId: string; name: string }) =>
      api.play(videoId, `${title} · ${name}`),
  });
  if (videos.length === 0) return null;
  return (
    <section data-cs="videos" aria-labelledby={heading} className="mt-8">
      <h2 id={heading} className="mb-3 font-display text-xl font-semibold">
        <Trans>Videos</Trans>
      </h2>
      <ul data-part="items" className="flex gap-3 overflow-x-auto pb-2">
        {videos.map((video, index) => {
          const number = index + 1;
          const name =
            video.name ??
            t({ message: `Video ${number}`, comment: "A game's video IGDB gives no name" });
          return (
            <li key={video.videoId} className="w-64 shrink-0 sm:w-72">
              <Button
                data-part="video"
                onPress={() => play.mutate({ videoId: video.videoId, name })}
                aria-label={t({
                  message: `Play ${name}`,
                  comment: "{name} is a game's video: Trailer, Gameplay video, …",
                })}
                className="group block w-full text-left outline-none"
              >
                <span className="relative block overflow-hidden rounded-control border border-border group-data-focus-visible:focus-ring-over group-data-hovered:border-border-strong">
                  <img
                    data-part="thumbnail"
                    src={videoThumbnail(video.videoId)}
                    alt=""
                    loading="lazy"
                    className="aspect-video w-full bg-surface-raised object-cover"
                  />
                  <span
                    data-part="play"
                    aria-hidden
                    className="absolute inset-0 m-auto grid size-12 place-items-center rounded-full bg-surface-base/75 text-fg group-data-hovered:bg-accent group-data-hovered:text-accent-fg"
                  >
                    <Play className="size-5 translate-x-px" />
                  </span>
                </span>
                <span
                  data-part="name"
                  className="mt-1.5 block truncate text-sm text-fg-muted group-data-hovered:text-fg"
                >
                  {name}
                </span>
              </Button>
            </li>
          );
        })}
      </ul>
      {play.isError && (
        <p role="alert" className="mt-2 text-sm text-danger">
          {play.error.message}
        </p>
      )}
    </section>
  );
}
