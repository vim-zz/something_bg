use std::collections::HashMap;

use log::debug;
use muda::Submenu;
use something_bg_core::config::{Config, SectionKind};
use something_bg_core::scheduler::{TaskScheduler, cron_to_human_readable, format_last_run};
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem};

/// Holds references to menu items so we can update their checked state / labels.
pub struct MenuHandles {
    pub tunnels: Vec<TunnelHandle>,
    pub commands: Vec<CommandHandle>,
    pub tasks: Vec<TaskHandle>,
    pub reload_config_id: Option<MenuId>,
    pub open_config_id: MenuId,
    pub disconnect_all: MenuItem,
    pub disconnect_all_id: MenuId,
    pub about_id: MenuId,
    pub view_history_id: Option<MenuId>,
    pub quit_id: MenuId,
}

pub struct TunnelHandle {
    pub id: MenuId,
    pub key: String,
    pub item: CheckMenuItem,
}

pub struct CommandHandle {
    pub id: MenuId,
    pub key: String,
}

pub struct TaskHandle {
    pub key: String,
    pub run_id: MenuId,
    pub next_run_item: MenuItem,
    pub last_run_item: MenuItem,
}

pub fn build_menu(
    config: &Config,
    scheduler: &TaskScheduler,
    show_reload: bool,
) -> (Menu, MenuHandles) {
    let menu = Menu::new();

    let mut tunnels = Vec::new();
    let mut commands = Vec::new();
    let mut tasks = Vec::new();
    let mut view_history_id = None;
    let last_command_section = config
        .sections
        .iter()
        .rposition(|section| section.kind == SectionKind::Command && !section.item_ids.is_empty());
    let mut rendered_section = false;

    for (section_index, section) in config.sections.iter().enumerate() {
        if section.item_ids.is_empty() {
            continue;
        }
        if rendered_section && let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
            debug!("failed to append section separator: {e}");
        }
        rendered_section = true;
        maybe_add_group_header(&menu, section.title.as_deref());

        for key in &section.item_ids {
            match section.kind {
                SectionKind::Tunnel => {
                    let Some(tunnel) = config.tunnel(key) else {
                        continue;
                    };
                    let item = CheckMenuItem::new(&tunnel.name, true, false, None);
                    let id = item.id();
                    if let Err(e) = menu.append(&item) {
                        debug!("failed to append tunnel item: {e}");
                    }
                    tunnels.push(TunnelHandle {
                        id: id.clone(),
                        key: key.clone(),
                        item: item.clone(),
                    });
                }
                SectionKind::Command => {
                    let Some(command) = config.command(key) else {
                        continue;
                    };
                    let item = MenuItem::new(&command.name, true, None);
                    let id = item.id().clone();
                    if let Err(e) = menu.append(&item) {
                        debug!("failed to append command item: {e}");
                    }
                    commands.push(CommandHandle {
                        id,
                        key: key.clone(),
                    });
                }
                SectionKind::ScheduledTask => {
                    let Some(task) = config.schedule(key) else {
                        continue;
                    };
                    let submenu = Submenu::new(&task.name, true);
                    let schedule_item = MenuItem::new(
                        format!("Schedule: {}", cron_to_human_readable(&task.cron_schedule)),
                        false,
                        None,
                    );
                    let next_run_item = MenuItem::new(
                        format!(
                            "Next run: {}",
                            format_last_run(&scheduler.get_task(key).and_then(|t| t.next_run))
                        ),
                        false,
                        None,
                    );
                    let last_run_item = MenuItem::new(
                        format!(
                            "Last run: {}",
                            format_last_run(&scheduler.get_task(key).and_then(|t| t.last_run))
                        ),
                        false,
                        None,
                    );
                    let run_now = MenuItem::new("Run Now", true, None);
                    let run_id = run_now.id().clone();
                    for item in [&schedule_item, &next_run_item, &last_run_item] {
                        if let Err(e) = submenu.append(item) {
                            debug!("failed to append scheduled-task detail: {e}");
                        }
                    }
                    if let Err(e) = submenu.append(&PredefinedMenuItem::separator()) {
                        debug!("failed to append submenu separator: {e}");
                    }
                    if let Err(e) = submenu.append(&run_now) {
                        debug!("failed to append run-now item: {e}");
                    }
                    if let Err(e) = menu.append(&submenu) {
                        debug!("failed to append task submenu: {e}");
                    }
                    tasks.push(TaskHandle {
                        key: key.clone(),
                        run_id,
                        next_run_item: next_run_item.clone(),
                        last_run_item: last_run_item.clone(),
                    });
                }
            }
        }

        if Some(section_index) == last_command_section {
            let view_history = MenuItem::new("View Command History", true, None);
            view_history_id = Some(view_history.id().clone());
            if let Err(e) = menu.append(&view_history) {
                debug!("failed to append view-history item: {e}");
            }
        }
    }

    if rendered_section && let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
        debug!("failed to append separator: {e}");
    }

    let reload_config_id = if show_reload {
        let reload = MenuItem::new("Reload Config", true, None);
        let id = reload.id().clone();
        if let Err(e) = menu.append(&reload) {
            debug!("failed to append reload-config item: {e}");
        }
        Some(id)
    } else {
        None
    };

    let open_config = MenuItem::new("Open Config Folder", true, None);
    let open_config_id = open_config.id().clone();
    if let Err(e) = menu.append(&open_config) {
        debug!("failed to append open-config item: {e}");
    }

    let disconnect_all = MenuItem::new("Disconnect All", false, None);
    let disconnect_all_id = disconnect_all.id().clone();
    if let Err(e) = menu.append(&disconnect_all) {
        debug!("failed to append disconnect-all item: {e}");
    }

    let about = MenuItem::new("About", true, None);
    let about_id = about.id().clone();
    if let Err(e) = menu.append(&about) {
        debug!("failed to append about item: {e}");
    }

    if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
        debug!("failed to append separator: {e}");
    }

    let quit = MenuItem::new("Quit Something in the Background", true, None);
    let quit_id = quit.id().clone();
    if let Err(e) = menu.append(&quit) {
        debug!("failed to append quit item: {e}");
    }

    (
        menu,
        MenuHandles {
            tunnels,
            commands,
            tasks,
            reload_config_id,
            open_config_id,
            disconnect_all,
            disconnect_all_id,
            about_id,
            view_history_id,
            quit_id,
        },
    )
}

fn maybe_add_group_header(menu: &Menu, header: Option<&str>) {
    if let Some(header) = header {
        let item = MenuItem::new(header, false, None);
        if let Err(e) = menu.append(&item) {
            debug!("failed to append group header: {e}");
        }
    }
}

/// Refresh "Last run" labels for all tasks. Call this periodically.
pub fn refresh_task_labels(handles: &MenuHandles, scheduler: &TaskScheduler) {
    let mut updated = 0;
    for handle in &handles.tasks {
        if let Some(task) = scheduler.get_task(&handle.key) {
            let next_label = format!("Next run: {}", format_last_run(&task.next_run));
            let label = format!("Last run: {}", format_last_run(&task.last_run));
            handle.next_run_item.set_text(&next_label);
            handle.last_run_item.set_text(&label);
            updated += 1;
        }
    }
    if updated > 0 {
        debug!("Updated last-run labels for {updated} scheduled tasks");
    }
}

/// Convenience map for looking up actions by id.
pub fn build_id_lookup(handles: &MenuHandles) -> HashMap<MenuId, MenuAction> {
    let mut map = HashMap::new();
    for t in &handles.tunnels {
        map.insert(t.id.clone(), MenuAction::ToggleTunnel(t.key.clone()));
    }
    for c in &handles.commands {
        map.insert(c.id.clone(), MenuAction::RunCommand(c.key.clone()));
    }
    for t in &handles.tasks {
        map.insert(t.run_id.clone(), MenuAction::RunTask(t.key.clone()));
    }
    map.insert(handles.about_id.clone(), MenuAction::About);
    if let Some(id) = &handles.reload_config_id {
        map.insert(id.clone(), MenuAction::ReloadConfig);
    }
    map.insert(handles.open_config_id.clone(), MenuAction::OpenConfig);
    map.insert(handles.disconnect_all_id.clone(), MenuAction::DisconnectAll);
    if let Some(id) = &handles.view_history_id {
        map.insert(id.clone(), MenuAction::ViewHistory);
    }
    map.insert(handles.quit_id.clone(), MenuAction::Quit);
    map
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    ToggleTunnel(String),
    RunCommand(String),
    RunTask(String),
    About,
    ReloadConfig,
    OpenConfig,
    DisconnectAll,
    ViewHistory,
    Quit,
}
