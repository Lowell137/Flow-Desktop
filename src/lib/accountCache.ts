import { profileStorage } from "./storage/profileStorage";

const PREFIX = "account_cache_";
const KEYS = ["home", "playlists", "channels", "likes"] as const;
export type AccountCacheKey = (typeof KEYS)[number];

/** The account's last answer, shown at once while a fresh one loads. */
export function readAccountCache<T>(key: AccountCacheKey): T | null {
  try {
    const raw = profileStorage.getItem(PREFIX + key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

export function writeAccountCache(key: AccountCacheKey, value: unknown): void {
  try {
    profileStorage.setItem(PREFIX + key, JSON.stringify(value));
  } catch {
    // Only a faster start is lost; the next load still fetches.
  }
}

/** Signing out leaves none of the account's lists behind. */
export function clearAccountCache(): void {
  for (const key of KEYS) {
    try {
      profileStorage.removeItem(PREFIX + key);
    } catch {
      // Nothing more can be done about storage that refuses.
    }
  }
}
