//! First-open window size. The config opens the main window at 1618 x 874
//! (inner, logical): the owner's laptop layout, where the sidebar, the filter
//! panel, the center and the right panel all fit (narrower and the right
//! panel folds away). On a screen too small for that, the window opens
//! maximized instead of spilling past the work area.

use tauri::{Manager, Runtime};

/// True when the window's outer size does not fit the monitor's work area.
pub fn needs_maximize(outer: (u32, u32), work_area: (u32, u32)) -> bool {
    outer.0 > work_area.0 || outer.1 > work_area.1
}

pub fn fit_to_screen<R: Runtime>(app: &tauri::App<R>) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let (Ok(Some(m)), Ok(size)) = (w.current_monitor(), w.outer_size()) else {
        return;
    };
    let area = m.work_area().size;
    if needs_maximize((size.width, size.height), (area.width, area.height)) {
        let _ = w.maximize();
    }
}

#[cfg(test)]
mod tests {
    use super::needs_maximize;

    #[test]
    fn maximizes_only_when_the_window_spills() {
        // 1920x1080 at 100%: work area 1920x1032, window 1634x913 fits.
        assert!(!needs_maximize((1634, 913), (1920, 1032)));
        // 1366x768 laptop: does not fit.
        assert!(needs_maximize((1634, 913), (1366, 728)));
        // 1920x1080 at 150%: the logical window is 1.5x larger in pixels.
        assert!(needs_maximize((2451, 1370), (1920, 1032)));
    }
}
