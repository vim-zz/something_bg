//! Linux tray shell for something_bg.
//! Provides a status icon with toggles for tunnels and scheduled tasks.

mod app;
mod menu;
mod paths;

use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use ctrlc;
use env_logger;
use gtk::glib;
use log::{error, info, warn};
use something_bg_core::platform::AppPaths;
use tray_icon::menu::MenuEvent;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::app::AppState;
use crate::menu::{MenuAction, MenuHandles, build_id_lookup, build_menu, refresh_task_labels};

fn main() {
    env_logger::init();
    info!("starting something_bg (linux tray)");

    gtk::init().expect("failed to init GTK"); // required for tray-icon on Linux

    let (app_state, config) = AppState::new();
    let running = Arc::new(AtomicBool::new(true));

    let (active_icon, idle_icon) = build_icons();
    let (menu, handles) = build_menu(&config, app_state.scheduler.as_ref());
    let id_lookup = build_id_lookup(&handles);

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(idle_icon.clone())
        .with_tooltip("something_bg")
        .build()
        .expect("failed to create tray icon");

    // Ctrl+C cleanup
    {
        let tm = app_state.tunnel_manager.clone();
        let sched = app_state.scheduler.clone();
        let running = running.clone();
        ctrlc::set_handler(move || {
            info!("received signal, cleaning up tunnels and exiting");
            tm.cleanup();
            sched.stop();
            running.store(false, Ordering::SeqCst);
        })
        .expect("Error setting Ctrl-C handler");
    }

    let mut looper = EventLoop {
        tray_icon,
        handles,
        id_lookup,
        app_state,
        active_icon,
        idle_icon,
        running,
        last_task_refresh: Instant::now(),
    };

    looper.run();
}

struct EventLoop {
    tray_icon: TrayIcon,
    handles: MenuHandles,
    id_lookup: std::collections::HashMap<muda::MenuId, MenuAction>,
    app_state: AppState,
    active_icon: Icon,
    idle_icon: Icon,
    running: Arc<AtomicBool>,
    last_task_refresh: Instant,
}

impl EventLoop {
    fn run(&mut self) {
        info!("tray icon ready; entering event loop");

        while self.running.load(Ordering::SeqCst) {
            // Process menu events (non-blocking)
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                self.handle_menu_event(event.id);
            }

            // Periodically refresh task labels so "Last run" stays current
            if self.last_task_refresh.elapsed() > Duration::from_secs(15) {
                refresh_task_labels(&self.handles, self.app_state.scheduler.as_ref());
                self.last_task_refresh = Instant::now();
            }

            glib::idle_add_local_once(|| {}); // allow GTK to process pending work
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }

            thread::sleep(Duration::from_millis(50));
        }

        info!("exiting event loop; cleaning up");
        self.app_state.cleanup();
    }

    fn handle_menu_event(&mut self, id: muda::MenuId) {
        if let Some(action) = self.id_lookup.get(&id).cloned() {
            match action {
                MenuAction::ToggleTunnel(key) => {
                    self.toggle_tunnel(&key);
                }
                MenuAction::RunTask(key) => {
                    if let Err(e) = self.app_state.scheduler.run_task_now(&key) {
                        error!("task '{}' failed: {}", key, e);
                    } else {
                        refresh_task_labels(&self.handles, self.app_state.scheduler.as_ref());
                    }
                }
                MenuAction::About => {
                    open_about();
                }
                MenuAction::OpenConfig => open_config(&self.app_state.paths),
                MenuAction::Quit => {
                    self.running.store(false, Ordering::SeqCst);
                }
            }
        }
    }

    fn toggle_tunnel(&mut self, key: &str) {
        let is_active = {
            let active = self.app_state.tunnel_manager.active_tunnels.lock().unwrap();
            active.contains(key)
        };
        let any_active = self.app_state.tunnel_manager.toggle(key, !is_active);
        self.update_icon(any_active);
        self.update_checked_state(key, !is_active);
    }

    fn update_checked_state(&mut self, key: &str, checked: bool) {
        for handle in &self.handles.tunnels {
            if handle.key == key {
                handle.item.set_checked(checked);
            }
        }
    }

    fn update_icon(&mut self, any_active: bool) {
        let icon = if any_active {
            self.active_icon.clone()
        } else {
            self.idle_icon.clone()
        };
        if let Err(e) = self.tray_icon.set_icon(Some(icon)) {
            warn!("failed to update tray icon: {e}");
        }
    }
}

fn open_config(paths: &std::sync::Arc<crate::paths::LinuxPaths>) {
    let config_path = paths.config_path();
    let parent = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(config_path);

    info!("opening config folder at {:?}", parent);
    let result = Command::new("xdg-open").arg(&parent).spawn();
    if let Err(e) = result {
        warn!("xdg-open failed: {e}");
    }
}

fn open_about() {
    let url = "https://github.com/vim-zz/something_bg";
    info!("opening project page: {url}");
    if let Err(e) = Command::new("xdg-open").arg(url).spawn() {
        warn!("failed to open browser: {e}");
    }
}

fn build_icons() -> (Icon, Icon) {
    // Simple 16x16 solid dots; avoid extra assets on Linux
    let active = solid_icon([0x29, 0xb6, 0xf6, 0xff]); // blue-ish
    let idle = solid_icon([0x77, 0x77, 0x77, 0xff]); // gray
    (active, idle)
}

fn solid_icon(color: [u8; 4]) -> Icon {
    let (width, height) = (16, 16);
    let mut data = Vec::with_capacity(width * height * 4);
    for _ in 0..(width * height) {
        data.extend_from_slice(&color);
    }
    Icon::from_rgba(data, width as u32, height as u32).expect("failed to build icon")
}
