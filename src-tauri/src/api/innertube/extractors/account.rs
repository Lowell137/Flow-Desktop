//! The signed-in account's own lists and actions, read from and written to YouTube.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::{Value, json};

use super::explore::video_of;
use crate::api::innertube::InnertubeClient;
use crate::api::innertube::core::account;
use crate::api::innertube::core::clients;
use crate::api::innertube::core::playback_session::new_cpn;
use crate::api::innertube::json::continuation::continuation_token;
use crate::api::innertube::music::parse::continuation::from_continuations;
use crate::api::innertube::music::parse::items::parse_yt_item;
use crate::errors::{AppError, AppResult};
use crate::models::music::{AlbumItem, PlaylistItem, SongItem, YTItem};
use crate::models::video::VideoSummary;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountFeed {
    pub videos: Vec<VideoSummary>,
    pub continuation: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountChannel {
    pub id: String,
    pub name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountMusic {
    pub songs: Vec<SongItem>,
    pub albums: Vec<AlbumItem>,
    pub playlists: Vec<PlaylistItem>,
    pub continuation: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub name: Option<String>,
    pub photo_url: Option<String>,
}

fn music_browse_id(kind: &str) -> AppResult<&'static str> {
    match kind {
        "songs" => Ok("VLLM"),
        "albums" => Ok("FEmusic_liked_albums"),
        "playlists" => Ok("FEmusic_liked_playlists"),
        _ => Err(AppError::Validation(format!("Unknown account library {kind}"))),
    }
}

/// Shelves YouTube adds beside a library or list: suggestions, not the account's own items.
const SUGGESTION_SHELVES: [&str; 2] = ["musicCarouselShelfRenderer", "musicShelfRenderer"];

fn collect_music(node: &Value, music: &mut AccountMusic) {
    match parse_yt_item(node) {
        Some(YTItem::Song(song)) => return music.songs.push(song),
        Some(YTItem::Album(album)) => return music.albums.push(album),
        Some(YTItem::Playlist(mut playlist)) => {
            playlist.id = playlist.id.trim_start_matches("VL").to_string();
            // Episodes for Later is a podcast list, which the music pages don't open.
            if playlist.id != "SE" {
                music.playlists.push(playlist);
            }
            return;
        }
        Some(_) => return,
        None => {}
    }
    if let Some(token) = continuation_token(node).or_else(|| from_continuations(node)) {
        music.continuation = Some(token);
    }
    match node {
        Value::Array(items) => items.iter().for_each(|item| collect_music(item, music)),
        Value::Object(fields) => fields
            .iter()
            .filter(|(key, _)| !SUGGESTION_SHELVES.contains(&key.as_str()))
            .for_each(|(_, field)| collect_music(field, music)),
        _ => {}
    }
}

fn https_url(url: &str) -> String {
    url.strip_prefix("//").map_or(url.to_string(), |rest| format!("https://{rest}"))
}

fn parse_profile(response: &Value) -> AccountProfile {
    let header = &response["actions"][0]["openPopupAction"]["popup"]["multiPageMenuRenderer"]["header"]
        ["activeAccountHeaderRenderer"];
    AccountProfile {
        name: header["accountName"]["simpleText"]
            .as_str()
            .or_else(|| header["accountName"]["runs"][0]["text"].as_str())
            .map(ToOwned::to_owned),
        photo_url: header["accountPhoto"]["thumbnails"]
            .as_array()
            .and_then(|thumbnails| thumbnails.last())
            .and_then(|thumbnail| thumbnail["url"].as_str())
            .map(https_url),
    }
}

fn browse_id(kind: &str) -> AppResult<&'static str> {
    match kind {
        "home" => Ok("FEwhat_to_watch"),
        "history" => Ok("FEhistory"),
        "liked" => Ok("VLLL"),
        _ => Err(AppError::Validation(format!("Unknown account feed {kind}"))),
    }
}

/// Account pages nest their cards differently per page, so this walks the whole
/// response instead of naming each page's layout.
fn collect_feed(node: &Value, feed: &mut AccountFeed, seen: &mut HashSet<String>) {
    if let Some(video) = video_of(node) {
        if seen.insert(video.id.clone()) {
            feed.videos.push(video);
        }
        return;
    }
    if let Some(token) = continuation_token(node) {
        feed.continuation = Some(token);
        return;
    }
    match node {
        Value::Array(items) => items.iter().for_each(|item| collect_feed(item, feed, seen)),
        Value::Object(fields) => fields.values().for_each(|field| collect_feed(field, feed, seen)),
        _ => {}
    }
}

fn parse_feed(response: &Value) -> AccountFeed {
    let mut feed = AccountFeed::default();
    collect_feed(response, &mut feed, &mut HashSet::new());
    feed
}

fn channel_of(renderer: &Value) -> Option<AccountChannel> {
    let id = renderer["channelId"].as_str().filter(|id| !id.is_empty())?;
    let title = &renderer["title"];
    let name = title["simpleText"]
        .as_str()
        .or_else(|| title["runs"][0]["text"].as_str())?;
    let avatar_url = renderer["thumbnail"]["thumbnails"]
        .as_array()
        .and_then(|thumbnails| thumbnails.last())
        .and_then(|thumbnail| thumbnail["url"].as_str())
        .map(https_url);
    Some(AccountChannel {
        id: id.to_string(),
        name: name.to_string(),
        avatar_url,
    })
}

fn collect_channels(node: &Value, channels: &mut Vec<AccountChannel>, token: &mut Option<String>) {
    for key in ["channelRenderer", "gridChannelRenderer"] {
        if let Some(channel) = node.get(key).and_then(channel_of) {
            channels.push(channel);
            return;
        }
    }
    if let Some(found) = continuation_token(node) {
        *token = Some(found);
        return;
    }
    match node {
        Value::Array(items) => items.iter().for_each(|item| collect_channels(item, channels, token)),
        Value::Object(fields) => fields.values().for_each(|field| collect_channels(field, channels, token)),
        _ => {}
    }
}

/// The query parameters that make a tracking URL count this playback, as yt-dlp
/// marks a video watched.
fn watched_url(base: &str, cpn: &str) -> Option<reqwest::Url> {
    let mut url = reqwest::Url::parse(base).ok()?;
    // Account cookies are only ever sent to YouTube's own hosts.
    let host = url.host_str()?;
    if url.scheme() != "https" || !(host == "youtube.com" || host.ends_with(".youtube.com")) {
        return None;
    }
    url.query_pairs_mut().append_pair("ver", "2").append_pair("cpn", cpn);
    Some(url)
}

impl InnertubeClient {
    pub async fn account_feed(&self, kind: &str, continuation: Option<&str>) -> AppResult<AccountFeed> {
        let mut payload = match continuation {
            Some(token) => json!({ "continuation": token }),
            None => json!({ "browseId": browse_id(kind)? }),
        };
        let response = self.post_innertube("browse", &clients::WEB, &mut payload).await?;
        Ok(parse_feed(&response))
    }

    pub async fn account_music(&self, kind: &str, continuation: Option<&str>) -> AppResult<AccountMusic> {
        let mut payload = match continuation {
            Some(token) => json!({ "continuation": token }),
            None => json!({ "browseId": music_browse_id(kind)? }),
        };
        let response = self
            .post_music("browse", &clients::WEB_REMIX, &mut payload, None)
            .await?;
        let mut music = AccountMusic::default();
        // Past the library grid, the section list only serves YouTube's suggestions.
        if response["continuationContents"]["sectionListContinuation"].is_object() {
            return Ok(music);
        }
        collect_music(&response, &mut music);
        tracing::info!(
            kind,
            songs = music.songs.len(),
            albums = music.albums.len(),
            playlists = music.playlists.len(),
            "Account library page read"
        );
        Ok(music)
    }

    pub async fn account_profile(&self) -> AppResult<AccountProfile> {
        let mut payload = json!({});
        let response = self
            .post_innertube("account/account_menu", &clients::WEB, &mut payload)
            .await?;
        Ok(parse_profile(&response))
    }

    pub async fn account_channels(&self) -> AppResult<Vec<AccountChannel>> {
        let mut channels = Vec::new();
        let mut payload = json!({ "browseId": "FEchannels" });
        // shortcut: stops at 20 pages of about 100 channels, raise if an account outgrows it.
        for _ in 0..20 {
            let response = self.post_innertube("browse", &clients::WEB, &mut payload).await?;
            let mut token = None;
            collect_channels(&response, &mut channels, &mut token);
            let Some(token) = token else { break };
            payload = json!({ "continuation": token });
        }
        Ok(channels)
    }

    pub async fn account_set_like(&self, video_id: &str, liked: bool) -> AppResult<()> {
        let endpoint = if liked { "like/like" } else { "like/removelike" };
        let mut payload = json!({ "target": { "videoId": video_id } });
        self.post_innertube(endpoint, &clients::WEB, &mut payload).await?;
        Ok(())
    }

    pub async fn account_dislike(&self, video_id: &str) -> AppResult<()> {
        let mut payload = json!({ "target": { "videoId": video_id } });
        self.post_innertube("like/dislike", &clients::WEB, &mut payload).await?;
        Ok(())
    }

    /// Saves a playlist or album to the account's library, or removes it from there.
    pub async fn account_set_saved(&self, playlist_id: &str, saved: bool) -> AppResult<()> {
        let endpoint = if saved { "like/like" } else { "like/removelike" };
        let mut payload = json!({ "target": { "playlistId": playlist_id } });
        self.post_innertube(endpoint, &clients::WEB, &mut payload).await?;
        Ok(())
    }

    pub async fn account_edit_playlist(
        &self,
        playlist_id: &str,
        add: &[String],
        remove: &[String],
    ) -> AppResult<()> {
        let actions: Vec<Value> = add
            .iter()
            .map(|id| json!({ "action": "ACTION_ADD_VIDEO", "addedVideoId": id }))
            .chain(
                remove
                    .iter()
                    .map(|id| json!({ "action": "ACTION_REMOVE_VIDEO_BY_VIDEO_ID", "removedVideoId": id })),
            )
            .collect();
        let mut payload = json!({ "playlistId": playlist_id, "actions": actions });
        self.post_innertube("browse/edit_playlist", &clients::WEB, &mut payload).await?;
        Ok(())
    }

    pub async fn account_create_playlist(&self, title: &str, video_ids: &[String]) -> AppResult<String> {
        let mut payload = json!({ "title": title, "videoIds": video_ids, "privacyStatus": "PRIVATE" });
        let response = self.post_innertube("playlist/create", &clients::WEB, &mut payload).await?;
        response["playlistId"]
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| AppError::Extractor("YouTube created no playlist".to_string()))
    }

    pub async fn account_set_subscribed(&self, channel_id: &str, subscribed: bool) -> AppResult<()> {
        let endpoint = if subscribed { "subscription/subscribe" } else { "subscription/unsubscribe" };
        let mut payload = json!({ "channelIds": [channel_id] });
        self.post_innertube(endpoint, &clients::WEB, &mut payload).await?;
        Ok(())
    }

    /// Puts `video_id` in the account's history, once per run: the player reports
    /// progress repeatedly and each report would be another view.
    pub async fn account_report_watched(&self, video_id: &str) {
        static REPORTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        let Some(session) = account::session() else { return };
        let first = REPORTED
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(video_id.to_string());
        if !first {
            return;
        }
        // Playback may come from an anonymous client, so the account asks for its own URLs.
        let mut payload = json!({ "videoId": video_id });
        let response = match self.post_innertube("player", &clients::WEB, &mut payload).await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(video_id, %error, "Could not read a video's tracking URLs");
                return;
            }
        };
        let cpn = new_cpn();
        let tracking = &response["playbackTracking"];
        let urls = ["videostatsPlaybackUrl", "videostatsWatchtimeUrl"]
            .into_iter()
            .filter_map(|key| tracking[key]["baseUrl"].as_str())
            .filter_map(|base| watched_url(base, &cpn));
        for url in urls {
            let sent = self
                .client
                .get(url)
                .header("User-Agent", clients::WEB.user_agent)
                .header("Cookie", session.cookie_header())
                .send()
                .await;
            if let Err(error) = sent {
                // The URL carries no cookie, but its query identifies the viewer.
                tracing::warn!(video_id, error = %error.without_url(), "Could not report a watch");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_page_reads_its_videos_once_and_its_next_page() {
        let page = json!({ "contents": [
            { "section": { "items": [
                { "videoRenderer": { "videoId": "aaaaaaaaaaa", "title": { "simpleText": "A" } } },
                { "videoRenderer": { "videoId": "aaaaaaaaaaa", "title": { "simpleText": "A" } } },
                { "continuationItemRenderer": { "continuationEndpoint": {
                    "continuationCommand": { "token": "NEXT" } } } }
            ] } }
        ] });
        let feed = parse_feed(&page);
        assert_eq!(feed.videos.len(), 1);
        assert_eq!(feed.continuation.as_deref(), Some("NEXT"));
    }

    #[test]
    fn channels_are_read_from_either_renderer() {
        let page = json!({ "items": [
            { "channelRenderer": { "channelId": "UC1", "title": { "simpleText": "One" },
              "thumbnail": { "thumbnails": [{ "url": "//yt3.ggpht.com/a" }] } } },
            { "gridChannelRenderer": { "channelId": "UC2", "title": { "runs": [{ "text": "Two" }] } } }
        ] });
        let (mut channels, mut token) = (Vec::new(), None);
        collect_channels(&page, &mut channels, &mut token);
        assert_eq!(channels[0].avatar_url.as_deref(), Some("https://yt3.ggpht.com/a"));
        assert_eq!(channels[1].name, "Two");
    }

    #[test]
    fn cookies_never_go_to_a_tracking_url_off_youtube() {
        assert!(watched_url("https://s.youtube.com/api/stats/playback?a=1", "c").is_some());
        assert!(watched_url("https://youtube.com.evil.test/x", "c").is_none());
        assert!(watched_url("http://s.youtube.com/x", "c").is_none());
    }

    #[test]
    fn the_profile_comes_from_the_account_menu_header() {
        let menu = json!({ "actions": [{ "openPopupAction": { "popup": { "multiPageMenuRenderer": { "header": {
            "activeAccountHeaderRenderer": {
                "accountName": { "simpleText": "lowell" },
                "accountPhoto": { "thumbnails": [{ "url": "small" }, { "url": "large" }] }
            } } } } } }] });
        assert_eq!(
            parse_profile(&menu),
            AccountProfile { name: Some("lowell".into()), photo_url: Some("large".into()) }
        );
        assert_eq!(parse_profile(&json!({})), AccountProfile::default());
    }

    #[test]
    fn a_library_keeps_real_playlists_without_their_browse_prefix() {
        let row = |id: &str| json!({ "musicTwoRowItemRenderer": {
            "title": { "runs": [{ "text": "List" }] },
            "navigationEndpoint": { "browseEndpoint": { "browseId": id,
                "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": {
                    "pageType": "MUSIC_PAGE_TYPE_PLAYLIST" } } } },
            "subtitle": { "runs": [] },
            "thumbnailRenderer": {}
        } });
        let page = json!([row("VLPLabc"), row("VLSE"), { "musicCarouselShelfRenderer": { "contents": [row("VLPLother")] } }]);
        let mut music = AccountMusic::default();
        collect_music(&page, &mut music);
        assert_eq!(music.playlists.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["PLabc"]);
    }

    #[test]
    fn only_known_libraries_are_browsed() {
        assert!(music_browse_id("albums").is_ok());
        assert!(music_browse_id("FEanything").is_err());
    }

    #[test]
    fn only_known_feeds_are_browsed() {
        assert!(browse_id("history").is_ok());
        assert!(browse_id("FEanything").is_err());
    }
}
