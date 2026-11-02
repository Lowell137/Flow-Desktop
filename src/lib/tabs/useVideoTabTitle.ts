import { useEffect, useState } from "react";
import { getVideoDetails } from "../api/youtube";

const titles = new Map<string, Promise<string | null>>();

/** A background tab's video has played nowhere yet, so its name is asked for once per video. */
export function useVideoTabTitle(videoId: string | null | undefined): string | null {
  const [title, setTitle] = useState<{ videoId: string; text: string } | null>(null);

  useEffect(() => {
    if (!videoId) return;
    let request = titles.get(videoId);
    if (!request) {
      request = getVideoDetails(videoId).then(
        (details) => details.title || null,
        () => {
          titles.delete(videoId);
          return null;
        },
      );
      titles.set(videoId, request);
    }
    let disposed = false;
    void request.then((text) => {
      if (!disposed && text) setTitle({ videoId, text });
    });
    return () => {
      disposed = true;
    };
  }, [videoId]);

  return title && title.videoId === videoId ? title.text : null;
}
