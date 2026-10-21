//! Tauri commands of the Settings, Notifications, Telegram panel. None of
//! them returns the token or the chat id.

use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use super::official::{self, is_telegram_link};
use super::tasks::{desk_live, now_ms, official_code, official_send, reset_cursors, save_settings, send};
use super::{delete_token, new_link_code, notify, write_token, BotToken, Mode, PendingLink, Prefs, TelegramManager, TelegramStatus, LINK_TTL_MS};
use crate::app::keychain::OsKeychain;
use crate::membership::MembershipManager;
use crate::tvlink::api::with_session;

fn view(mgr: &TelegramManager) -> TelegramStatus {
    mgr.lock().view(now_ms())
}

/// The panel's read. In official mode it also asks the server (the panel
/// polls every 3 s while a code waits): a new link turns notifications on
/// from "now"; a chat unlinked on the server is forgotten here too.
#[tauri::command]
pub async fn telegram_status(app: AppHandle, mgr: State<'_, TelegramManager>, membership: State<'_, MembershipManager>) -> Result<TelegramStatus, String> {
    if mgr.lock().mode() == Mode::Official {
        refresh_official(&app, &mgr, &membership).await;
    }
    Ok(view(&mgr))
}

async fn refresh_official(app: &AppHandle, mgr: &TelegramManager, membership: &MembershipManager) {
    let base = membership.base_url();
    let read = with_session(membership, |token| {
        let base = base.as_str();
        async move { mgr.official.status(base, &token).await }
    })
    .await
    .map_err(official_code);
    let save = {
        let mut g = mgr.lock();
        match read {
            Ok(v) => {
                g.official_signed_in = Some(true);
                g.official_available = Some(v.available);
                g.official_bot = Some(v.bot.clone());
                g.official_check_error = None;
                let before = g.settings.clone();
                if v.linked {
                    if g.official_linked(v.chat_label, v.linked_at) && g.mode() == Mode::Official {
                        if let Err(e) = reset_cursors(app, &mut g) {
                            g.last_error = Some((e, false));
                        }
                    }
                } else if g.settings.official_linked {
                    g.clear_official();
                }
                (g.settings != before).then(|| g.settings.clone())
            }
            Err(e) if e.code() == official::SIGNED_OUT => {
                g.official_signed_in = Some(false);
                g.official_check_error = None;
                None
            }
            Err(e) => {
                g.official_signed_in = Some(true);
                g.official_check_error = Some(e.code());
                None
            }
        }
    };
    if let Some(settings) = save {
        let _ = save_settings(app, &settings);
    }
}

/// Chooses the bot. Nothing waiting carries over, and the new mode starts
/// from "now" (no backlog).
#[tauri::command]
pub async fn telegram_set_mode(app: AppHandle, mgr: State<'_, TelegramManager>, mode: Mode) -> Result<TelegramStatus, String> {
    let settings = {
        let mut g = mgr.lock();
        if g.mode() == mode && g.settings.mode.is_some() {
            return Ok(g.view(now_ms()));
        }
        g.settings.mode = Some(mode);
        g.link = None;
        g.official_link = None;
        g.last_error = None;
        if let Err(e) = reset_cursors(&app, &mut g) {
            g.generation += 1;
            g.last_error = Some((e, false));
        }
        g.settings.clone()
    };
    {
        let mut ob = mgr.outbox.lock().await;
        ob.clear();
        let _ = ob.take_dropped();
    }
    mgr.lock().official_queued = 0;
    save_settings(&app, &settings)?;
    Ok(view(&mgr))
}

/// Official bot: asks the server for a one-time code and its t.me link.
#[tauri::command]
pub async fn telegram_official_link(mgr: State<'_, TelegramManager>, membership: State<'_, MembershipManager>) -> Result<TelegramStatus, String> {
    if mgr.lock().mode() != Mode::Official {
        return Err("tgNotOfficial".into());
    }
    let base = membership.base_url();
    let m = mgr.inner();
    let link = with_session(membership.inner(), |token| {
        let base = base.as_str();
        async move { m.official.link(base, &token, now_ms()).await }
    })
    .await
    .map_err(|e| official_code(e).code())?;
    {
        let mut g = mgr.lock();
        g.official_bot = Some(link.bot.clone());
        g.official_link = Some(link);
        g.official_signed_in = Some(true);
    }
    Ok(view(&mgr))
}

/// Opens the live code's t.me link in the system browser / Telegram. The
/// link comes from the server reply kept in Rust, never from the webview.
#[tauri::command]
pub fn telegram_official_open(app: AppHandle, mgr: State<TelegramManager>) -> Result<TelegramStatus, String> {
    let url = {
        let g = mgr.lock();
        g.official_link
            .as_ref()
            .filter(|l| l.expires_at_ms > now_ms() && is_telegram_link(&l.deep_link))
            .map(|l| l.deep_link.clone())
            .ok_or("tgLinkExpired")?
    };
    app.opener().open_url(url, None::<&str>).map_err(|_| "tgOpenFailed".to_string())?;
    Ok(view(&mgr))
}

/// Official bot: unlinks the chat on the server and here.
#[tauri::command]
pub async fn telegram_official_unlink(app: AppHandle, mgr: State<'_, TelegramManager>, membership: State<'_, MembershipManager>) -> Result<TelegramStatus, String> {
    let base = membership.base_url();
    let m = mgr.inner();
    with_session(membership.inner(), |token| {
        let base = base.as_str();
        async move { m.official.unlink(base, &token).await }
    })
    .await
    .map_err(|e| official_code(e).code())?;
    let settings = {
        let mut g = mgr.lock();
        g.clear_official();
        g.last_error = None;
        g.settings.clone()
    };
    {
        let mut ob = mgr.outbox.lock().await;
        ob.clear();
        let _ = ob.take_dropped();
    }
    mgr.lock().official_queued = 0;
    save_settings(&app, &settings)?;
    Ok(view(&mgr))
}

/// Checks the token with `getMe`, then keeps it in the OS keychain. A new
/// token may be another bot, so the chat link starts over.
#[tauri::command]
pub async fn telegram_set_token(app: AppHandle, mgr: State<'_, TelegramManager>, token: String) -> Result<TelegramStatus, String> {
    let parsed = BotToken::parse(&token);
    drop(token);
    let token = parsed?;
    let username = mgr.api.get_me(&token).await?;
    write_token(&OsKeychain, &token)?;
    let settings = {
        let mut g = mgr.lock();
        if g.token.as_ref() != Some(&token) {
            g.clear_link();
        }
        g.token = Some(token);
        g.settings.bot_username = Some(username);
        g.last_error = None;
        g.settings.clone()
    };
    save_settings(&app, &settings)?;
    Ok(view(&mgr))
}

/// The user's answer to an unreadable saved row: Telegram starts from the
/// defaults (not linked, nothing sent) and may be saved again. The kept copy
/// stays in the store under its own key.
#[tauri::command]
pub fn telegram_start_over(app: AppHandle, mgr: State<TelegramManager>) -> Result<TelegramStatus, String> {
    mgr.hold_unreadable.store(false, std::sync::atomic::Ordering::SeqCst);
    let settings = {
        let mut g = mgr.lock();
        g.settings_unreadable = None;
        g.last_error = None;
        g.settings.clone()
    };
    save_settings(&app, &settings)?;
    Ok(view(&mgr))
}

/// Deletes the token and forgets the chat. The category choices stay.
#[tauri::command]
pub fn telegram_remove(app: AppHandle, mgr: State<TelegramManager>) -> Result<TelegramStatus, String> {
    delete_token(&OsKeychain);
    let settings = {
        let mut g = mgr.lock();
        g.token = None;
        g.clear_link();
        g.settings.bot_username = None;
        g.last_error = None;
        g.last_sent_at = None;
        g.settings.clone()
    };
    save_settings(&app, &settings)?;
    Ok(view(&mgr))
}

/// Starts linking: a fresh one-time code for `/start <code>`.
#[tauri::command]
pub fn telegram_link_code(mgr: State<TelegramManager>) -> Result<TelegramStatus, String> {
    {
        let mut g = mgr.lock();
        if g.token.is_none() {
            return Err("telegramNoToken".into());
        }
        g.link = Some(PendingLink { code: new_link_code(), expires_at_ms: now_ms() + LINK_TTL_MS });
    }
    Ok(view(&mgr))
}

/// Saves the switches. Turning notifications on starts from "now".
#[tauri::command]
pub fn telegram_set_prefs(app: AppHandle, mgr: State<TelegramManager>, prefs: Prefs) -> Result<TelegramStatus, String> {
    let settings = {
        let mut g = mgr.lock();
        let official = g.mode() == Mode::Official;
        // The official bot relays no commands: that switch is kept as it was.
        let commands = if official { g.settings.commands_enabled } else { prefs.commands_enabled };
        if (prefs.enabled || (commands && !official)) && !g.linked() {
            return Err("telegramNotLinked".into());
        }
        if prefs.enabled && !g.settings.enabled {
            reset_cursors(&app, &mut g)?;
        }
        g.settings.enabled = prefs.enabled;
        g.settings.commands_enabled = commands;
        g.settings.categories = prefs.categories;
        g.settings.clone()
    };
    save_settings(&app, &settings)?;
    Ok(view(&mgr))
}

/// Sends one test message to the linked chat. Official mode: straight to
/// the relay (not through the outbox); a success clears "blocked".
#[tauri::command]
pub async fn telegram_test(app: AppHandle, mgr: State<'_, TelegramManager>) -> Result<TelegramStatus, String> {
    if mgr.lock().mode() == Mode::Official {
        let enabled = {
            let g = mgr.lock();
            if !g.settings.official_linked {
                return Err("telegramNotLinked".into());
            }
            g.settings.enabled
        };
        let text = format!(
            "{}\nTest message. Notifications are {}.",
            notify::header(desk_live(&app)),
            if enabled { "on" } else { "off" }
        );
        let membership = app.state::<MembershipManager>();
        let r = official_send(mgr.inner(), membership.inner(), &text).await;
        let save = {
            let mut g = mgr.lock();
            match &r {
                Ok(()) => {
                    g.last_sent_at = Some(now_ms());
                    g.official_blocked = false;
                    g.official_unavailable = false;
                    g.last_error = None;
                    None
                }
                Err(e) => {
                    let code = e.code();
                    g.official_unavailable = code == official::UNAVAILABLE;
                    if code == official::BLOCKED {
                        g.official_blocked = true;
                    }
                    if code == official::NOT_LINKED {
                        g.clear_official();
                        Some(g.settings.clone())
                    } else {
                        None
                    }
                }
            }
        };
        if let Some(settings) = save {
            save_settings(&app, &settings)?;
        }
        r.map_err(|e| e.code())?;
        return Ok(view(&mgr));
    }
    let (token, chat, enabled, commands) = {
        let g = mgr.lock();
        let token = g.token.clone().ok_or("telegramNoToken")?;
        let chat = g.settings.chat_id.ok_or("telegramNotLinked")?;
        (token, chat, g.settings.enabled, g.settings.commands_enabled)
    };
    let on = |b: bool| if b { "on" } else { "off" };
    let text = format!(
        "{}\nTest message. Notifications are {}. Commands are {}.",
        notify::header(desk_live(&app)),
        on(enabled),
        on(commands)
    );
    send(&app, &token, chat, &text).await?;
    Ok(view(&mgr))
}

#[cfg(test)]
mod tests {
    /// No command hands the token back: the status DTO has no field for it
    /// and no command returns a String.
    #[test]
    fn no_command_returns_the_token() {
        let src = include_str!("commands.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        for line in body.lines().filter(|l| l.contains("pub fn") || l.contains("pub async fn")) {
            assert!(line.contains("-> Result<TelegramStatus, String>"), "{line}");
        }
        let status = include_str!("mod.rs");
        let start = status.find("pub struct TelegramStatus {").unwrap();
        let dto = &status[start..start + status[start..].find('}').unwrap()];
        assert!(!dto.contains("token:") && !dto.contains("chat_id"), "{dto}");
    }
}
