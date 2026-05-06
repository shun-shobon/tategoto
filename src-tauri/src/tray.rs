use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::{
    TRAY_ID,
    model::{SharedAppState, TranscriptionStatus},
    transcription::{start_recording, stop_recording},
};

pub(crate) fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", "Start", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &start, &stop, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .title("○ 待機中")
        .tooltip("Tategoto")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "start" => {
                if let Some(state) = app.try_state::<SharedAppState>() {
                    let app = app.clone();
                    let state = state.inner().clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = start_recording(app, state).await;
                    });
                }
            }
            "stop" => {
                if let Some(state) = app.try_state::<SharedAppState>() {
                    let app = app.clone();
                    let state = state.inner().clone();
                    tauri::async_runtime::spawn(async move {
                        stop_recording(app, state).await;
                    });
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub(crate) async fn update_tray_status(app: &AppHandle, state: &SharedAppState) {
    let status = {
        let model = state.model.lock().await;
        model.status.clone()
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let (marker, label) = tray_status_label(&status);
        let _ = tray.set_title(Some(format!("{marker} {label}")));
        let _ = tray.set_tooltip(Some(format!("Tategoto: {label}")));
    }
}

fn tray_status_label(status: &TranscriptionStatus) -> (&'static str, &'static str) {
    match status {
        TranscriptionStatus::Idle => ("○", "待機中"),
        TranscriptionStatus::Recording => ("●", "録音中"),
        TranscriptionStatus::StoppedWithError => ("!", "エラー"),
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
