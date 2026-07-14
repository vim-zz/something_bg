# Something In The Background

A native menu bar utility for managing background processes, SSH tunnels, and scheduled tasks.

<img src="something_bg_menu.png" alt="Menu Bar Screenshot" width="600">

## Features

- Tiny native macOS app with a Rust core (less than 1MB)
- Run any script or CLI task without keeping a terminal open
- Fire-and-forget one-time commands (silent, with notification, or in a terminal)
- Auto-discover scripts from a directory
- Run scripts on a schedule without cron, or launchd
- Controlled from the menu bar
- Cross-platform support (macOS, Linux, Windows)
- Everything is configured with one simple config file

## Installation

### macOS

#### Install from GitHub Releases

Download the latest macOS zip from the [GitHub Releases page](https://github.com/vim-zz/something_bg/releases), unzip it, then move `Something in the Background.app` to `/Applications`.

> [!IMPORTANT]
> Because the GitHub-built app is currently not signed or notarized, macOS may show a warning that the app is "corrupted" or say it should be moved to the Trash. If that happens, run:

```bash
xattr -dr com.apple.quarantine "/Applications/Something in the Background.app"
```

#### Build from source

**Step 1: Install Rust** (skip if already installed)
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

**Step 2: Install build tools**
```bash
xcode-select --install
cargo install cargo-bundle
```

**Step 3: Build and install**
```bash
git clone https://github.com/vim-zz/something_bg.git
cd something_bg
./scripts/bundle-macos.sh
cp -r "target/release/bundle/osx/Something in the Background.app" /Applications/
```

Launch from Applications or run: `open "/Applications/Something in the Background.app"`

### Linux

#### Install from GitHub Releases

Download the latest Linux tarball from the [GitHub Releases page](https://github.com/vim-zz/something_bg/releases), then extract and run the binary:

```bash
tar -xzf something_bg-linux-x86_64-unknown-linux-gnu.tar.gz
chmod +x something_bg_linux
./something_bg_linux
```

Move `something_bg_linux` somewhere on your `PATH` if you want to launch it more easily later.

#### Build from source

**Prerequisites** (Ubuntu/Debian):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
sudo apt update
sudo apt install build-essential pkg-config libayatana-appindicator3-dev libgtk-3-dev libxdo-dev
```

**Build and run**:
```bash
git clone https://github.com/vim-zz/something_bg.git
cd something_bg
cargo run -p something_bg_linux --release
```

### Windows

**Build on Windows**:
```powershell
# Install Rust from https://rustup.rs/
git clone https://github.com/vim-zz/something_bg.git
cd something_bg
cargo build -p something_bg_windows --release
.\target\release\something_bg_windows.exe
```

## Configuration

Configuration is stored in `~/.config/something_bg/config.toml` (created on first run).

### Example

```toml
version = 2

[environment]
path = "/bin:/usr/bin:/usr/local/bin:/opt/homebrew/bin"

[[sections]]
id = "database"
title = "DATABASE"
icon = "sf:cylinder.fill"
kind = "tunnel"

[[sections.items]]
id = "database-prod"
name = "PROD"
start = ["ssh", "-N", "-L", "5432:localhost:5432", "user@server.com"]
stop = ["pkill", "-f", "user@server.com"]

[[sections]]
id = "kubernetes"
title = "KUBERNETES"
icon = "sf:cloud.fill"
kind = "tunnel"

[[sections.items]]
id = "k8s-service"
name = "API Service"
start = ["kubectl", "port-forward", "svc/api", "8080:8080"]
stop = ["pkill", "-f", "svc/api"]

[[sections]]
id = "personal"
title = "PERSONAL"
icon = "sf:person.fill"
kind = "command"

[[sections.items]]
id = "fix-quarantine"
name = "Fix Whisperer Quarantine"
run = ["xattr", "-dr", "com.apple.quarantine", "/Applications/whisperer.app"]

[[sections.items]]
id = "deploy"
name = "Deploy"
run = ["bash", "/Users/me/scripts/deploy.sh"]
output = "terminal"

[[sections]]
id = "scheduled"
title = "SCHEDULED"
icon = "sf:clock.fill"
kind = "scheduled-task"

[[sections.items]]
id = "daily-backup"
name = "Daily Backup"
run = ["/usr/local/bin/backup.sh"]
cron = "0 6 * * *"
```

### Fields

- `version` — Config schema version; the current version is `2`.
- `sections` — Ordered menu sections. The app inserts separators between them.
- Section `id` — Stable identifier, unique across sections.
- Section `title` and `icon` — Optional visible heading and SF Symbol.
- Section `kind` — `"tunnel"`, `"command"`, or `"scheduled-task"`.
- Item `id` — Stable identifier, unique within its kind.
- Item `name` — Display name.
- Tunnel `start` and `stop` — Executable followed by its exact argument list.
- Command `run` — Executable followed by arguments; `output` controls output handling.
- Scheduled-task `run` and `cron` — Command and five-field cron expression.

The order of `[[sections]]` and `[[sections.items]]` entries is the menu order. Commands are executed directly; use `["bash", "-c", "..."]` when shell syntax such as pipes or `&&` is required.

Legacy unversioned files and `version = 1` files are migrated automatically. The original is retained as `config.toml.v1.bak`, while `config.toml` is rewritten in the current format.

### One-Time Commands

Run any command with a single click from the menu bar. Each command has a configurable `output` mode:

| Mode | Behavior | Best for |
|------|----------|----------|
| `silent` (default) | Fire and forget, no output | Instant commands (`xattr`, `pkill`) |
| `notify` | Run in background, show notification on completion with last 5 lines of output | Scripts that take seconds to minutes |
| `terminal` | Open a terminal window with live output | Long/interactive scripts, debugging |

```toml
version = 2

[[sections]]
id = "utilities"
title = "UTILITIES"
kind = "command"

[[sections.items]]
id = "fix-quarantine"
name = "Fix Quarantine"
run = ["xattr", "-dr", "com.apple.quarantine", "/Applications/myapp.app"]
# output defaults to "silent"

[[sections.items]]
id = "backup"
name = "Run Backup"
run = ["bash", "/usr/local/bin/backup.sh"]
output = "notify"

[[sections.items]]
id = "deploy"
name = "Deploy"
run = ["bash", "/usr/local/bin/deploy.sh"]
output = "terminal"
```

### Scripts Directory

Auto-discover shell scripts from a directory. All `*.sh` files appear in the menu under a "Scripts" header, sorted alphabetically. Default output mode is `notify`.

```toml
version = 2

[scripts]
directory = "~/.config/something_bg/scripts"
output = "notify"
section = "scripts"

[[sections]]
id = "scripts"
title = "SCRIPTS"
icon = "sf:terminal.fill"
kind = "command"
```

Filenames are title-cased for display: `delete-logs.sh` → "Delete Logs".

### Scheduled Tasks

Common cron patterns:

- `0 * * * *` — Every hour
- `*/15 * * * *` — Every 15 minutes
- `0 6 * * *` — Daily at 6am
- `0 9 * * 1` — Mondays at 9am

### SF Symbols (macOS icons)

Common symbols for section `icon`:

- `sf:cylinder.fill` — Database
- `sf:shippingbox.fill` — Cache/Redis
- `sf:cloud.fill` — Cloud/Kubernetes
- `sf:server.rack` — Server
- `sf:network` — Network
- `sf:clock.fill` — Scheduled tasks
- `sf:hammer.fill` — Development

Browse all symbols at [developer.apple.com/sf-symbols](https://developer.apple.com/sf-symbols/) or use the SF Symbols app.

Reload the configuration from the tray menu after editing the file.

## License

MIT
