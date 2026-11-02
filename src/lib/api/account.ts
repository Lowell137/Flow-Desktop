import { invokeBackend } from "./errors";
import { isTauriEnv } from "./env";
import type { AlbumItem, PlaylistItem, SongItem } from "../../types/music";
import type { VideoSummary } from "../../types/video";

export type AccountFeedKind = "home" | "history" | "liked";

export interface AccountFeed {
  videos: VideoSummary[];
  continuation: string | null;
}

export interface AccountChannel {
  id: string;
  name: string;
  avatarUrl: string | null;
}

let status: Promise<boolean> | null = null;

/** The session is fixed for the run, so one answer serves every caller. */
export function isSignedIn(): Promise<boolean> {
  status ??= isTauriEnv().then((tauri) => (tauri ? invokeBackend<boolean>("account_status") : false));
  return status;
}

/** The app restarts once signed in; false means the window was closed first. */
export const signInWithGoogle = () => invokeBackend<boolean>("account_google_sign_in");

/** Moves the file into the keychain, deleting it, and restarts the app. */
export const importCookiesFile = (path: string) => invokeBackend<void>("account_import_cookies", { path });

export const signOut = () => invokeBackend<void>("account_sign_out");

export const getAccountFeed = (kind: AccountFeedKind, continuation?: string | null) =>
  invokeBackend<AccountFeed>("account_feed", { kind, continuation: continuation ?? null });

export interface AccountMusic {
  songs: SongItem[];
  albums: AlbumItem[];
  playlists: PlaylistItem[];
  continuation: string | null;
}

export interface AccountProfile {
  name: string | null;
  photoUrl: string | null;
}

export const getAccountMusic = (kind: "songs" | "albums" | "playlists", continuation?: string | null) =>
  invokeBackend<AccountMusic>("account_music", { kind, continuation: continuation ?? null });

export const getAccountProfile = () => invokeBackend<AccountProfile>("account_profile");

export const getAccountChannels = () => invokeBackend<AccountChannel[]>("account_channels");

export const setAccountLike = (videoId: string, liked: boolean) =>
  invokeBackend<void>("account_set_like", { videoId, liked });

export const setAccountDislike = (videoId: string) =>
  invokeBackend<void>("account_dislike", { videoId });

export const setAccountSaved = (playlistId: string, saved: boolean) =>
  invokeBackend<void>("account_set_saved", { playlistId, saved });

export const editAccountPlaylist = (playlistId: string, add: string[], remove: string[]) =>
  invokeBackend<void>("account_edit_playlist", { playlistId, add, remove });

export const createAccountPlaylist = (title: string, videoIds: string[]) =>
  invokeBackend<string>("account_create_playlist", { title, videoIds });

export const setAccountSubscribed = (channelId: string, subscribed: boolean) =>
  invokeBackend<void>("account_set_subscribed", { channelId, subscribed });
