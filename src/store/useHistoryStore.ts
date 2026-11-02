import { create } from "zustand";

import { getAccountFeed, isSignedIn, type AccountFeed } from "../lib/api/account";
import { clearWatchHistory, deleteWatchRecord, getWatchHistory } from "../lib/api/db";
import type { WatchHistoryRecord } from "../types/db";

export const HISTORY_PAGE_SIZE = 200;
/** Fired after watch history changed behind the stores, for example by a sync. */
export const HISTORY_UPDATED_EVENT = "flow:history-updated";

interface HistoryState {
  records: WatchHistoryRecord[];
  loaded: boolean;
  loading: boolean;
  loadingMore: boolean;
  hasMore: boolean;
  offset: number;
  load: (force?: boolean) => Promise<void>;
  loadMore: () => Promise<void>;
  remove: (videoId: string) => Promise<void>;
  clear: () => Promise<void>;
}

let inFlight = false;
let accountContinuation: string | null = null;

// YouTube's history page gives no watch time per video.
function accountRecords(feed: AccountFeed): WatchHistoryRecord[] {
  accountContinuation = feed.continuation;
  const watchDate = new Date().toISOString();
  return feed.videos.map((video) => ({
    videoId: video.id,
    title: video.title,
    channelName: video.channelName,
    channelId: video.channelId,
    watchDate,
    watchDurationSeconds: 0,
    totalDurationSeconds: video.durationSeconds,
  }));
}

export const useHistoryStore = create<HistoryState>((set, get) => ({
  records: [],
  loaded: false,
  loading: false,
  loadingMore: false,
  hasMore: false,
  offset: 0,
  load: async (force = false) => {
    if (inFlight) return;
    if (get().loaded && !force) return;
    inFlight = true;
    set({ loading: true });
    try {
      if (await isSignedIn()) {
        const records = accountRecords(await getAccountFeed("history"));
        set({ records, offset: records.length, hasMore: accountContinuation !== null, loaded: true });
        return;
      }
      const records = await getWatchHistory(HISTORY_PAGE_SIZE, 0);
      set({
        records,
        offset: records.length,
        hasMore: records.length === HISTORY_PAGE_SIZE,
        loaded: true,
      });
    } catch (error) {
      console.error("Failed to fetch history", error);
    } finally {
      inFlight = false;
      set({ loading: false });
    }
  },
  loadMore: async () => {
    if (inFlight) return;
    inFlight = true;
    set({ loadingMore: true });
    try {
      const signedIn = await isSignedIn();
      const records = signedIn
        ? accountRecords(await getAccountFeed("history", accountContinuation))
        : await getWatchHistory(HISTORY_PAGE_SIZE, get().offset);
      set((state) => {
        const seen = new Set(state.records.map((r) => r.videoId));
        const fresh = records.filter((r) => !seen.has(r.videoId));
        return {
          records: fresh.length > 0 ? [...state.records, ...fresh] : state.records,
          offset: state.offset + records.length,
          hasMore: signedIn ? accountContinuation !== null : records.length === HISTORY_PAGE_SIZE,
        };
      });
    } catch (error) {
      console.error("Failed to fetch more history", error);
    } finally {
      inFlight = false;
      set({ loadingMore: false });
    }
  },
  remove: async (videoId) => {
    // shortcut: removing from the account's history isn't wired, add it with `feedback` tokens.
    if (await isSignedIn()) return;
    await deleteWatchRecord(videoId);
    set((state) => ({
      records: state.records.filter((item) => item.videoId !== videoId),
      offset: Math.max(0, state.offset - 1),
    }));
  },
  clear: async () => {
    if (await isSignedIn()) return;
    await clearWatchHistory();
    set({ records: [], offset: 0, hasMore: false });
  },
}));
