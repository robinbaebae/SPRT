mod claude;
mod devlog;
mod git;
mod storage;

use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition,
};

#[tauri::command]
fn update_tray_title(app: tauri::AppHandle, title: String) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_title(Some(&title));
    }
}

#[tauri::command]
fn open_dashboard(app: tauri::AppHandle) {
    if let Some(p) = app.get_webview_window("popover") {
        let _ = p.hide();
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            claude::get_stats_cache,
            claude::get_active_sessions,
            claude::get_project_usage,
            claude::get_realtime_stats,
            claude::get_rate_limits,
            devlog::generate_devlog,
            devlog::get_devlog,
            devlog::list_devlogs,
            devlog::get_git_activity,
            update_tray_title,
            open_dashboard,
        ])
        .setup(|app| {
            // Hide from dock, show only in menu bar
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            // Right-click menu
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let show = MenuItemBuilder::with_id("show", "Open Dashboard").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .separator()
                .item(&quit)
                .build()?;

            // Build tray with dummy icon, then remove it
            let icon_data: Vec<u8> = vec![0; 4];
            let icon = Image::new(&icon_data, 1, 1);

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .title("—")
                .tooltip("SPRT")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app_handle, event| match event.id().0.as_str() {
                    "quit" => app_handle.exit(0),
                    "show" => {
                        // Hide popover, show main dashboard
                        if let Some(p) = app_handle.get_webview_window("popover") {
                            let _ = p.hide();
                        }
                        if let Some(w) = app_handle.get_webview_window("main") {
                            let _ = w.center();
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray_icon, event| {
                    match event {
                    TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } => {
                        let app = tray_icon.app_handle();
                        // Hide popover, open dashboard
                        if let Some(p) = app.get_webview_window("popover") {
                            let _ = p.hide();
                        }
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.center();
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } => {
                        let app = tray_icon.app_handle();

                        // Toggle popover
                        if let Some(w) = app.get_webview_window("popover") {
                            if w.is_visible().unwrap_or(false) {
                                let _ = w.hide();
                                return;
                            }

                            // Position below tray icon, centered
                            let tray_x = match rect.position {
                                tauri::Position::Physical(p) => p.x as f64,
                                tauri::Position::Logical(p) => p.x,
                            };
                            let tray_y = match rect.position {
                                tauri::Position::Physical(p) => p.y as f64,
                                tauri::Position::Logical(p) => p.y,
                            };
                            let tray_h = match rect.size {
                                tauri::Size::Physical(s) => s.height as f64,
                                tauri::Size::Logical(s) => s.height,
                            };

                            let pop_w = 250.0;
                            let x = (tray_x - pop_w / 2.0).max(8.0);
                            let y = tray_y + tray_h + 4.0;

                            let _ = w.set_position(PhysicalPosition::new(x as i32, y as i32));
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    _ => {}
                    }
                })
                .build(app)?;

            // Set tray icon from bundled resource
            if let Some(tray) = app.tray_by_id("main-tray") {
                let icon_bytes = include_bytes!("../icons/tray-icon.png");
                if let Ok(img) = image::load_from_memory(icon_bytes) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let tray_img = Image::new(rgba.as_raw(), w, h);
                    let _ = tray.set_icon(Some(tray_img));
                    let _ = tray.set_icon_as_template(true);
                }
            }

            // Show main dashboard on launch & hide on close (instead of destroy)
            if let Some(main_win) = app.get_webview_window("main") {
                let _ = main_win.center();
                let _ = main_win.show();
                let _ = main_win.set_focus();

                let mw = main_win.clone();
                main_win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = mw.hide();
                    }
                });
            }

            // Popover: hide on focus lost
            if let Some(popover) = app.get_webview_window("popover") {
                let pop = popover.clone();
                popover.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        let _ = pop.hide();
                    }
                });
            }

            // Tray title updater — reads from rate limit cache every 5s
            let tray_app = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let title = claude::get_cached_utilization()
                        .map(|pct| format!("{}%", (pct * 100.0).round() as u32))
                        .unwrap_or_else(|| "—".to_string());
                    if let Some(tray) = tray_app.tray_by_id("main-tray") {
                        let _ = tray.set_title(Some(&title));
                    }
                }
            });

            // File watcher
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                use notify::{Config, RecursiveMode, Watcher};
                let (tx, rx) = std::sync::mpsc::channel();
                let mut watcher =
                    notify::RecommendedWatcher::new(tx, Config::default()).unwrap();

                if let Some(cd) = dirs::home_dir().map(|h| h.join(".claude")) {
                    let sf = cd.join("stats-cache.json");
                    if sf.exists() {
                        let _ = watcher.watch(&sf, RecursiveMode::NonRecursive);
                    }
                    let pd = cd.join("projects");
                    if pd.exists() {
                        let _ = watcher.watch(&pd, RecursiveMode::Recursive);
                    }
                }

                loop {
                    match rx.recv() {
                        Ok(_) => { let _ = app_handle.emit("claude-data-changed", ()); }
                        Err(_) => break,
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
