//! Tauri commands of the Settings, Notifications, Telegram panel. None of
//! them returns the token or the chat id.

use tauri::{AppHandle, State};

use super::tasks::{desk_live, now_ms, reset_cursors, save_settings, send};
use super::{delete_token, new_link_code, notify, write_token, BotToken, PendingLink, Prefs, TelegramManager, TelegramStatus, LINK_TTL_MS};
use crate::app::keychain::OsKeychain;

fn view(mgr: &TelegramManager) -> TelegramStatus {
    mgr.lock().view(now_ms())
}

#[tauri::command]
pub fn telegram_status(mgr: State<TelegramManager>) -> Result<TelegramStatus, String> {
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
        if (prefs.enabled || prefs.commands_enabled) && g.settings.chat_id.is_none() {
            return Err("telegramNotLinked".into());
        }
        if prefs.enabled && !g.settings.enabled {
            reset_cursors(&app, &mut g)?;
        }
        g.settings.enabled = prefs.enabled;
        g.settings.commands_enabled = prefs.commands_enabled;
        g.settings.categories = prefs.categories;
        g.settings.clone()
    };
    save_settings(&app, &settings)?;
    Ok(view(&mgr))
}

/// Sends one test message to the linked chat.
#[tauri::command]
pub async fn telegram_test(app: AppHandle, mgr: State<'_, TelegramManager>) -> Result<TelegramStatus, String> {
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
