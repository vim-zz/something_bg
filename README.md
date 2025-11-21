# Something in the Background

A lightweight native macOS utility for running scripts and commands in the background - powered by Rust.

`something_bg` is a tiny menu/tray app (macOS, Linux, Windows) designed to take any script or command you already use and run it quietly in the background. No terminals to keep open, no remembering where scripts live, no complicated setup.

If you’ve ever left a Terminal window open _“just to keep a command running”_, this app is for you.

<img align="right" src="something_bg.png" alt="Menu Bar Screenshot" width="400">

## Repository layout

- `core/` — platform-agnostic logic (config, scheduler, tunnel management).
- `app-macos/` — macOS shell (tray UI, oslog, wake detection) that links to `core`.
- `app-linux/` — Linux tray shell that links to `core`.
- `app-windows/` — Windows tray shell that links to `core`.

## Features

- Tiny native macOS app with a Rust core (less than 1MB)
- Run any script or CLI task without keeping a terminal open
- Run scripts on a schedule without cron, or launchd
- Controlled from the menu bar
- Everything is configured with one simple config file

## Installation

### macOS (native bundle)

Prereqs:
- Rust and Cargo (install via [rustup](https://rustup.rs/))
- Xcode Command Line Tools
- cargo-bundle (`cargo install cargo-bundle`)

1. Clone the repository:
```bash
git clone https://github.com/vim-zz/something_bg.git
cd something_bg
```

2. Build and bundle the macOS application (run from repo root):
```bash
cargo bundle --release --bin something_bg
```

3. Install or run
```bash
# run from build location
open "target/release/bundle/osx/Something in the Background.app"
# or install
cp -r "target/release/bundle/osx/Something in the Background.app" /Applications/
```

### Linux (tray shell)

Prereqs (Ubuntu/Debian):
```bash
sudo apt install libayatana-appindicator3-dev libgtk-3-dev
```

Run the tray app:
```bash
cargo run -p something_bg_linux
```

You’ll get a status icon in the system tray:
- Click tunnels to toggle them on/off (checkboxes reflect state)
- “Run now” under each scheduled task runs it immediately
- “Open config folder” opens `~/.config/something_bg/`
- “Quit” cleans up tunnels and stops the scheduler

#### Run cargo check in Docker (no local GTK deps needed)
```bash
./scripts/linux-cargo-check.sh
```
The script builds an Ubuntu-based image with GTK/AppIndicator dev packages and runs `cargo check -p something_bg_linux` inside it. Pass extra cargo args after the script if needed.

### Windows (tray shell, cross-compiling from macOS/Linux)

Prereqs: Docker.

Cross-check/build with the bundled cargo-xwin image:
```bash
./scripts/windows-cargo-check.sh
```
This builds `something_bg_windows` for `x86_64-pc-windows-msvc` using the Windows SDK bundled in the Docker image. Pass extra cargo args after the script if needed.

To run on Windows natively, build on Windows with a Rust toolchain and the `tray-icon` deps:
```powershell
cargo build -p something_bg_windows --release
```
Then launch the produced `target\release\something_bg_windows.exe`.

## Configuration

The app loads all menu items from `~/.config/something_bg/config.toml`. On first run, it creates this file with example configurations.

### Example Configuration

```toml
# Custom PATH for command execution
path = "/bin:/usr/bin:/usr/local/bin:/opt/homebrew/bin"

[tunnels]

# SSH tunnel with port forwarding
[tunnels.database-prod]
name = "PROD"
command = "ssh"
args = ["-N", "-L", "5432:localhost:5432", "user@server.com"]
kill_command = "pkill"
kill_args = ["-f", "user@server.com"]
group_header = "DATABASE"           # Optional: Section header
group_icon = "sf:cylinder.fill"     # Optional: SF Symbol icon

[tunnels.database-dev]
name = "DEV"
command = "ssh"
args = ["-N", "-L", "5432:localhost:5432", "dev@server.com"]
kill_command = "pkill"
kill_args = ["-f", "dev@server.com"]
separator_after = true              # Optional: Add separator after this item

# Kubernetes port forwarding
[tunnels.k8s-service]
name = "Service"
command = "kubectl"
args = ["port-forward", "svc/my-service", "8080:8080"]
kill_command = "pkill"
kill_args = ["-f", "svc/my-service"]
group_header = "KUBERNETES"
group_icon = "sf:cloud.fill"

# Development server
[tunnels.dev-server]
name = "Dev Server"
command = "npm"
args = ["run", "dev"]
kill_command = "pkill"
kill_args = ["-f", "npm.*dev"]
group_header = "DEVELOPMENT"
group_icon = "sf:hammer.fill"
separator_after = true

[schedules.daily-backup]
name = "Daily Backup"
command = "/usr/local/bin/backup.sh"
args = []
cron_schedule = "0 6 * * *"          # Every day at 6:00 AM
group_header = "SCHEDULED TASKS"
group_icon = "sf:clock.fill"
```

### Configuration Fields

**For Tunnels:**
- `name`: Display name in the menu
- `command` + `args`: Command to start the service
- `kill_command` + `kill_args`: Command to stop the service

**For Scheduled Tasks:**
- `name`: Display name in the menu
- `command` + `args`: Command to execute
- `cron_schedule`: Cron expression for scheduling (e.g., "0 6 * * *")

**Optional fields (both types):**
- `group_header`: Section title (e.g., "DATABASE", "SCHEDULED TASKS")
- `group_icon`: SF Symbol name for the header (e.g., "sf:cylinder.fill", "sf:clock.fill")
- `separator_after`: Add a visual separator line after this item

### Scheduled Tasks

Schedule commands to run automatically using cron syntax. Perfect for backups, health checks, or recurring maintenance tasks.

```toml
[schedules.daily-backup]
name = "Daily Backup"
command = "/usr/local/bin/backup.sh"
args = []
cron_schedule = "0 6 * * *"          # Every day at 6:00 AM
group_header = "SCHEDULED TASKS"
group_icon = "sf:clock.fill"

[schedules.hourly-health-check]
name = "API Health Check"
command = "curl"
args = ["-f", "https://api.example.com/health"]
cron_schedule = "0 * * * *"          # Every hour

[schedules.weekly-cleanup]
name = "Weekly Cleanup"
command = "/usr/local/bin/cleanup.sh"
args = ["--deep"]
cron_schedule = "0 3 * * 0"          # Every Sunday at 3:00 AM
separator_after = true
```

**Cron Schedule Format:** `minute hour day_of_month month day_of_week`

Common examples:
- `0 * * * *` - Every hour
- `*/15 * * * *` - Every 15 minutes
- `0 6 * * *` - Every day at 6:00 AM
- `0 9 * * 1` - Every Monday at 9:00 AM
- `0 0 1 * *` - First day of every month at midnight

### SF Symbols

Group icons use SF Symbols (built-in macOS icons). Common symbols:
- `sf:cylinder.fill` - Database
- `sf:shippingbox.fill` - Cache/Redis
- `sf:chart.bar.fill` - Analytics
- `sf:cloud.fill` - Cloud/Kubernetes
- `sf:server.rack` - Server
- `sf:network` - Network
- `sf:clock.fill` - Scheduled tasks
- `sf:calendar` - Time-based operations

Browse all symbols at [developer.apple.com/sf-symbols](https://developer.apple.com/sf-symbols/) or use the SF Symbols app.

Restart the app to pick up configuration changes.

## License

MIT
