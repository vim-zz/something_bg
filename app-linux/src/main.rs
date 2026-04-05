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
use gtk::prelude::*;
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
        last_tick: Instant::now(),
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
    last_tick: Instant,
}

impl EventLoop {
    fn run(&mut self) {
        info!("tray icon ready; entering event loop");

        while self.running.load(Ordering::SeqCst) {
            let elapsed = self.last_tick.elapsed();
            if elapsed > Duration::from_secs(30) {
                self.on_wake(elapsed);
            }
            self.last_tick = Instant::now();

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

    fn on_wake(&mut self, gap: Duration) {
        info!(
            "Detected system wake (gap {:?}); recycling active tunnels",
            gap
        );
        self.app_state.handle_wake();

        let any_active = self.app_state.tunnel_manager.has_active_tunnels();
        self.update_icon(any_active);
        refresh_task_labels(&self.handles, self.app_state.scheduler.as_ref());
    }

    fn handle_menu_event(&mut self, id: muda::MenuId) {
        if let Some(action) = self.id_lookup.get(&id).cloned() {
            match action {
                MenuAction::ToggleTunnel(key) => {
                    self.toggle_tunnel(&key);
                }
                MenuAction::RunCommand(key) => {
                    if let Err(e) = self.app_state.command_runner.run_by_key(&key) {
                        error!("command '{}' failed: {}", key, e);
                    }
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
                MenuAction::DisconnectAll => {
                    self.disconnect_all();
                }
                MenuAction::ViewHistory => {
                    open_history(&self.app_state.command_runner);
                }
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
        self.set_disconnect_all_enabled(any_active);
    }

    fn disconnect_all(&mut self) {
        let active_keys: Vec<String> = {
            let active = self.app_state.tunnel_manager.active_tunnels.lock().unwrap();
            active.iter().cloned().collect()
        };

        if active_keys.is_empty() {
            return;
        }

        for key in active_keys {
            self.app_state.tunnel_manager.toggle(&key, false);
            self.update_checked_state(&key, false);
        }

        let any_active = self.app_state.tunnel_manager.has_active_tunnels();
        self.update_icon(any_active);
        self.set_disconnect_all_enabled(any_active);
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

    fn set_disconnect_all_enabled(&self, enabled: bool) {
        self.handles.disconnect_all.set_enabled(enabled);
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

fn open_history(command_runner: &something_bg_core::command::CommandRunner) {
    if let Some(path) = command_runner.history_path() {
        if path.exists() {
            info!("opening command history at {:?}", path);
            if let Err(e) = Command::new("xdg-open").arg(path).spawn() {
                warn!("failed to open history: {e}");
            }
        } else {
            info!("no command history yet");
        }
    }
}

fn open_about() {
    info!("opening about dialog");

    let dialog = gtk::AboutDialog::new();
    dialog.set_modal(true);
    dialog.set_program_name("Something in the Background");
    dialog.set_version(Some(env!("CARGO_PKG_VERSION")));
    dialog.set_comments(Some(
        "Menu bar app for SSH tunnels, Kubernetes port forwarding, and scheduled background commands.",
    ));
    dialog.set_website(Some("https://github.com/vim-zz/something_bg"));
    dialog.set_website_label(Some("Project page"));
    dialog.set_authors(&["vim-zz"]);
    dialog.connect_response(|dialog, _| {
        dialog.close();
    });
    dialog.show_all();
    dialog.present();
}

fn build_icons() -> (Icon, Icon) {
    // Render a circular status icon with transparent padding so Linux trays do not
    // show it as a filled square.
    let active = filled_circle_icon([0x29, 0xb6, 0xf6, 0xff]); // blue-ish
    let idle = ring_icon([0x77, 0x77, 0x77, 0xff]); // gray outline
    (active, idle)
}

fn filled_circle_icon(color: [u8; 4]) -> Icon {
    let (width, height) = (16usize, 16usize);
    let center = ((width as f32 - 1.0) / 2.0, (height as f32 - 1.0) / 2.0);
    let radius = 5.25f32;
    let mut data = Vec::with_capacity(width * height * 4);

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - center.0;
            let dy = y as f32 - center.1;
            let distance = (dx * dx + dy * dy).sqrt();

            let alpha = if distance <= radius {
                color[3]
            } else if distance <= radius + 1.0 {
                let falloff = 1.0 - (distance - radius);
                (color[3] as f32 * falloff.clamp(0.0, 1.0)).round() as u8
            } else {
                0
            };

            data.extend_from_slice(&[color[0], color[1], color[2], alpha]);
        }
    }

    Icon::from_rgba(data, width as u32, height as u32).expect("failed to build icon")
}

fn ring_icon(color: [u8; 4]) -> Icon {
    let (width, height) = (16usize, 16usize);
    let center = ((width as f32 - 1.0) / 2.0, (height as f32 - 1.0) / 2.0);
    let outer_radius = 5.2f32;
    let inner_radius = 4.2f32;
    let mut data = Vec::with_capacity(width * height * 4);

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - center.0;
            let dy = y as f32 - center.1;
            let distance = (dx * dx + dy * dy).sqrt();

            let alpha = if distance >= inner_radius && distance <= outer_radius {
                color[3]
            } else if distance >= inner_radius - 1.0 && distance < inner_radius {
                let falloff = 1.0 - (inner_radius - distance);
                (color[3] as f32 * falloff.clamp(0.0, 1.0)).round() as u8
            } else if distance > outer_radius && distance <= outer_radius + 1.0 {
                let falloff = 1.0 - (distance - outer_radius);
                (color[3] as f32 * falloff.clamp(0.0, 1.0)).round() as u8
            } else {
                0
            };

            data.extend_from_slice(&[color[0], color[1], color[2], alpha]);
        }
    }

    Icon::from_rgba(data, width as u32, height as u32).expect("failed to build icon")
}
