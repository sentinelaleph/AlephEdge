//! Tauri command surface for membership. Returns only the secret-free
//! `MembershipView`; tokens never cross IPC.

use tauri::State;

use super::model::MembershipView;
use super::MembershipManager;

#[tauri::command]
pub async fn membership_login(
    membership: State<'_, MembershipManager>,
    email: String,
    password: String,
) -> Result<MembershipView, String> {
    membership.login(&email, &password).await
}

#[tauri::command]
pub fn membership_logout(membership: State<MembershipManager>) -> MembershipView {
    membership.logout();
    membership.view()
}

#[tauri::command]
pub fn membership_status(membership: State<MembershipManager>) -> MembershipView {
    membership.view()
}

#[tauri::command]
pub async fn membership_refresh(
    membership: State<'_, MembershipManager>,
) -> Result<MembershipView, String> {
    membership.refresh_status().await
}

/// Called once on app launch: silently restores a session from the OS
/// keychain's persisted refresh token, if any, so the user isn't asked to
/// sign in again on every launch.
#[tauri::command]
pub async fn membership_restore(
    membership: State<'_, MembershipManager>,
) -> Result<MembershipView, String> {
    Ok(membership.restore().await)
}
