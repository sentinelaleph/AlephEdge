//! Tauri command surface for the signal client.

use tauri::{AppHandle, State};

use super::model::{Signal, SignalHealthInfo};
use super::SignalManager;

#[tauri::command]
pub fn signal_connect(app: AppHandle, signal: State<SignalManager>) {
    signal.connect(app);
}

#[tauri::command]
pub fn signal_disconnect(signal: State<SignalManager>) {
    signal.disconnect();
}

#[tauri::command]
pub fn signal_recent(signal: State<SignalManager>) -> Vec<Signal> {
    signal.recent()
}

#[tauri::command]
pub fn signal_health(signal: State<SignalManager>) -> SignalHealthInfo {
    signal.health()
}
