import { useCallback, useEffect, useState } from "react";
import { readAccountCache, writeAccountCache } from "./accountCache";
import { getAccountFeed, isSignedIn } from "./api/account";
import type { VideoSummary } from "../types/video";

/** YouTube's own Home for a signed-in session; `signedIn` is null until known. */
export function useAccountHome() {
  const [signedIn, setSignedIn] = useState<boolean | null>(null);
  const [videos, setVideos] = useState<VideoSummary[]>(() => readAccountCache<VideoSummary[]>("home") ?? []);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);

  const load = useCallback(async () => {
    // With a saved Home on screen the new one replaces it quietly.
    setLoading(readAccountCache("home") === null);
    setFailed(false);
    try {
      const fresh = (await getAccountFeed("home")).videos;
      setVideos(fresh);
      writeAccountCache("home", fresh);
    } catch (error) {
      console.error("Failed to load the account's Home", error);
      setFailed(true);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void isSignedIn().then((value) => {
      setSignedIn(value);
      if (value) void load();
    });
  }, [load]);

  return { signedIn, videos, loading, failed, refresh: load };
}
