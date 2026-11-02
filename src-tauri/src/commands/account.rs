//! The signed-in YouTube account, as the frontend's stores see it.

use std::time::Duration;

use tauri::{AppHandle, Manager, State, Url, WebviewUrl, WebviewWindowBuilder};

use crate::api::innertube::core::account;
use crate::api::innertube::extractors::account::{
    AccountChannel, AccountFeed, AccountMusic, AccountProfile,
};
use crate::security::validation::{validate_page_token, validate_video_id};
use crate::errors::{AppError, ErrorResponse};
use crate::security::keychain::off_thread;
use crate::services::youtube_service::YoutubeService;

const LOGIN_WINDOW: &str = "google-login";
const LOGIN_URL: &str = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fwww.youtube.com%2F";
const MAX_COOKIES_FILE_BYTES: u64 = 1024 * 1024;
const LOGIN_POLL: Duration = Duration::from_secs(1);

fn failure(message: impl Into<String>) -> ErrorResponse {
    ErrorResponse::from(AppError::Validation(message.into()))
}

async fn save_session(session: account::Session) -> Result<(), ErrorResponse> {
    off_thread(move || account::store_session(&session))
        .await
        .map_err(|error| failure(error.to_string()))
}

/// Reading the keychain can wait on an unlock prompt, so it stays off the main thread.
#[tauri::command]
pub async fn account_status() -> Result<bool, ErrorResponse> {
    off_thread(|| Ok(account::session().is_some()))
        .await
        .map_err(|error| failure(error.to_string()))
}

/// Moves a cookies file into the keychain: the plaintext is deleted once the keychain holds it.
#[tauri::command]
pub async fn account_import_cookies(app: AppHandle, path: String) -> Result<(), ErrorResponse> {
    let size = tokio::fs::metadata(&path).await.map_err(|_| failure("Could not read that file"))?.len();
    if size > MAX_COOKIES_FILE_BYTES {
        return Err(failure("That file is too large to be a cookies file"));
    }
    let text = tokio::fs::read_to_string(&path).await.map_err(|_| failure("Could not read that file"))?;
    let session = account::Session::parse(&text)
        .ok_or_else(|| failure("That file has no signed-in YouTube session"))?;
    save_session(session).await?;
    tokio::fs::remove_file(&path)
        .await
        .map_err(|_| failure("Signed in, but the cookies file could not be deleted. Delete it yourself."))?;
    app.restart()
}

/// Google's own page in its own window, with no access to the app's commands. Answers false
/// when the window is closed without signing in. Its browsing data is removed afterwards, so
/// the keychain holds the only copy.
#[tauri::command]
pub async fn account_google_sign_in(app: AppHandle) -> Result<bool, ErrorResponse> {
    if let Some(window) = app.get_webview_window(LOGIN_WINDOW) {
        let _ = window.set_focus();
        return Ok(false);
    }
    let data_dir = app
        .path()
        .app_cache_dir()
        .map_err(|_| failure("Could not prepare the sign-in window"))?
        .join(LOGIN_WINDOW);
    let _ = tokio::fs::remove_dir_all(&data_dir).await;
    let login_url = Url::parse(LOGIN_URL).map_err(|_| failure("Could not open Google"))?;
    let youtube = Url::parse("https://www.youtube.com/").map_err(|_| failure("Could not open Google"))?;
    let window = WebviewWindowBuilder::new(&app, LOGIN_WINDOW, WebviewUrl::External(login_url))
        .title("Google")
        .inner_size(480.0, 720.0)
        .data_directory(data_dir.clone())
        .build()
        .map_err(|_| failure("Could not open the sign-in window"))?;

    let session = loop {
        tokio::time::sleep(LOGIN_POLL).await;
        if app.get_webview_window(LOGIN_WINDOW).is_none() {
            break None;
        }
        let Ok(cookies) = window.cookies_for_url(youtube.clone()) else {
            continue;
        };
        let found = account::Session::from_cookies(cookies.iter().map(|cookie| (cookie.name(), cookie.value())));
        if found.is_some() {
            break found;
        }
    };
    let signed_in = session.is_some();
    let saved = match session {
        Some(session) => save_session(session).await,
        None => Ok(()),
    };
    let _ = window.close();
    tokio::time::sleep(LOGIN_POLL).await;
    let _ = tokio::fs::remove_dir_all(&data_dir).await;
    saved?;
    if signed_in {
        app.restart();
    }
    Ok(false)
}

/// Forgets the session; the local library is back after the restart.
#[tauri::command]
pub async fn account_sign_out(app: AppHandle) -> Result<(), ErrorResponse> {
    off_thread(account::clear_session)
        .await
        .map_err(|error| failure(error.to_string()))?;
    app.restart()
}

#[tauri::command]
pub async fn account_feed(
    kind: String,
    continuation: Option<String>,
    youtube_service: State<'_, YoutubeService>,
) -> Result<AccountFeed, ErrorResponse> {
    if let Some(token) = continuation.as_deref() {
        validate_page_token(token).map_err(ErrorResponse::from)?;
    }
    youtube_service
        .account()
        .account_feed(&kind, continuation.as_deref())
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_music(
    kind: String,
    continuation: Option<String>,
    youtube_service: State<'_, YoutubeService>,
) -> Result<AccountMusic, ErrorResponse> {
    if let Some(token) = continuation.as_deref() {
        validate_page_token(token).map_err(ErrorResponse::from)?;
    }
    youtube_service
        .account()
        .account_music(&kind, continuation.as_deref())
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_profile(
    youtube_service: State<'_, YoutubeService>,
) -> Result<AccountProfile, ErrorResponse> {
    youtube_service
        .account()
        .account_profile()
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_channels(
    youtube_service: State<'_, YoutubeService>,
) -> Result<Vec<AccountChannel>, ErrorResponse> {
    youtube_service
        .account()
        .account_channels()
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_set_like(
    video_id: String,
    liked: bool,
    youtube_service: State<'_, YoutubeService>,
) -> Result<(), ErrorResponse> {
    validate_video_id(&video_id).map_err(ErrorResponse::from)?;
    youtube_service
        .account()
        .account_set_like(&video_id, liked)
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_dislike(
    video_id: String,
    youtube_service: State<'_, YoutubeService>,
) -> Result<(), ErrorResponse> {
    validate_video_id(&video_id).map_err(ErrorResponse::from)?;
    youtube_service
        .account()
        .account_dislike(&video_id)
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_set_saved(
    playlist_id: String,
    saved: bool,
    youtube_service: State<'_, YoutubeService>,
) -> Result<(), ErrorResponse> {
    youtube_service
        .account()
        .account_set_saved(&playlist_id, saved)
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_edit_playlist(
    playlist_id: String,
    add: Vec<String>,
    remove: Vec<String>,
    youtube_service: State<'_, YoutubeService>,
) -> Result<(), ErrorResponse> {
    for id in add.iter().chain(&remove) {
        validate_video_id(id).map_err(ErrorResponse::from)?;
    }
    youtube_service
        .account()
        .account_edit_playlist(&playlist_id, &add, &remove)
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_create_playlist(
    title: String,
    video_ids: Vec<String>,
    youtube_service: State<'_, YoutubeService>,
) -> Result<String, ErrorResponse> {
    for id in &video_ids {
        validate_video_id(id).map_err(ErrorResponse::from)?;
    }
    youtube_service
        .account()
        .account_create_playlist(&title, &video_ids)
        .await
        .map_err(ErrorResponse::from)
}

#[tauri::command]
pub async fn account_set_subscribed(
    channel_id: String,
    subscribed: bool,
    youtube_service: State<'_, YoutubeService>,
) -> Result<(), ErrorResponse> {
    youtube_service
        .account()
        .account_set_subscribed(&channel_id, subscribed)
        .await
        .map_err(ErrorResponse::from)
}
