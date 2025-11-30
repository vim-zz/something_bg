use std::collections::HashMap;

use log::debug;
use something_bg_core::config::{Config, ScheduledTaskConfig, TunnelConfig};
use something_bg_core::scheduler::{TaskScheduler, cron_to_human_readable, format_last_run};
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem};

pub struct MenuHandles {
    pub tunnels: Vec<TunnelHandle>,
    pub tasks: Vec<TaskHandle>,
    pub about_id: MenuId,
    pub open_config_id: MenuId,
    pub quit_id: MenuId,
}

pub struct TunnelHandle {
    pub id: MenuId,
    pub key: String,
    pub item: CheckMenuItem,
}

pub struct TaskHandle {
    pub key: String,
    pub run_id: MenuId,
    pub last_run_item: MenuItem,
}

pub fn build_menu(config: &Config, scheduler: &TaskScheduler) -> (Menu, MenuHandles) {
    let menu = Menu::new();

    let mut tunnels = Vec::new();
    for (key, tunnel) in &config.tunnels {
        add_group_header(&menu, tunnel);

        let item = CheckMenuItem::new(&tunnel.name, true, false, None);
        let id = item.id().clone();
        if let Err(e) = menu.append(&item) {
            debug!("failed to append tunnel item: {e}");
        }
        tunnels.push(TunnelHandle {
            id,
            key: key.clone(),
            item: item.clone(),
        });

        if tunnel.separator_after.unwrap_or(false) {
            if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
                debug!("failed to append separator: {e}");
            }
        }
    }

    if !config.schedules.is_empty() && !config.tunnels.is_empty() {
        if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
            debug!("failed to append separator: {e}");
        }
    }

    let mut tasks = Vec::new();
    for (key, task) in &config.schedules {
        add_group_header_for_task(&menu, task);

        let schedule_line = format!("Schedule: {}", cron_to_human_readable(&task.cron_schedule));
        let schedule_item = MenuItem::new(&schedule_line, false, None);
        if let Err(e) = menu.append(&schedule_item) {
            debug!("failed to append schedule label: {e}");
        }

        let last_run_item = MenuItem::new(
            &format!(
                "Last run: {}",
                format_last_run(&scheduler.get_task(key).map(|t| t.last_run).flatten())
            ),
            false,
            None,
        );
        if let Err(e) = menu.append(&last_run_item) {
            debug!("failed to append last-run label: {e}");
        }

        let run_now = MenuItem::new("Run now", true, None);
        let run_id = run_now.id().clone();
        if let Err(e) = menu.append(&run_now) {
            debug!("failed to append run-now item: {e}");
        }
        tasks.push(TaskHandle {
            key: key.clone(),
            run_id,
            last_run_item: last_run_item.clone(),
        });

        if task.separator_after.unwrap_or(false) {
            if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
                debug!("failed to append separator: {e}");
            }
        }
    }

    if !config.tunnels.is_empty() || !config.schedules.is_empty() {
        if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
            debug!("failed to append separator: {e}");
        }
    }

    let about = MenuItem::new("About", true, None);
    let about_id = about.id().clone();
    if let Err(e) = menu.append(&about) {
        debug!("failed to append about item: {e}");
    }

    let open_config = MenuItem::new("Open config folder", true, None);
    let open_config_id = open_config.id().clone();
    if let Err(e) = menu.append(&open_config) {
        debug!("failed to append open-config item: {e}");
    }

    let quit = MenuItem::new("Quit", true, None);
    let quit_id = quit.id().clone();
    if let Err(e) = menu.append(&quit) {
        debug!("failed to append quit item: {e}");
    }

    (
        menu,
        MenuHandles {
            tunnels,
            tasks,
            about_id,
            open_config_id,
            quit_id,
        },
    )
}

fn add_group_header(menu: &Menu, tunnel: &TunnelConfig) {
    if let Some(header) = &tunnel.group_header {
        let item = MenuItem::new(header, false, None);
        if let Err(e) = menu.append(&item) {
            debug!("failed to append group header: {e}");
        }
    }
}

fn add_group_header_for_task(menu: &Menu, task: &ScheduledTaskConfig) {
    if let Some(header) = &task.group_header {
        let item = MenuItem::new(header, false, None);
        if let Err(e) = menu.append(&item) {
            debug!("failed to append group header: {e}");
        }
    }
}

pub fn refresh_task_labels(handles: &MenuHandles, scheduler: &TaskScheduler) {
    let mut updated = 0;
    for handle in &handles.tasks {
        if let Some(task) = scheduler.get_task(&handle.key) {
            let label = format!("Last run: {}", format_last_run(&task.last_run));
            handle.last_run_item.set_text(&label);
            updated += 1;
        }
    }
    if updated > 0 {
        debug!("Updated last-run labels for {updated} scheduled tasks");
    }
}

pub fn build_id_lookup(handles: &MenuHandles) -> HashMap<MenuId, MenuAction> {
    let mut map = HashMap::new();
    for t in &handles.tunnels {
        map.insert(t.id.clone(), MenuAction::ToggleTunnel(t.key.clone()));
    }
    for t in &handles.tasks {
        map.insert(t.run_id.clone(), MenuAction::RunTask(t.key.clone()));
    }
    map.insert(handles.about_id.clone(), MenuAction::About);
    map.insert(handles.open_config_id.clone(), MenuAction::OpenConfig);
    map.insert(handles.quit_id.clone(), MenuAction::Quit);
    map
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    ToggleTunnel(String),
    RunTask(String),
    About,
    OpenConfig,
    Quit,
}
