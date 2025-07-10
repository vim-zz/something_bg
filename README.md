# Something in the Background

![Menu Bar Screenshot](menubar.webp)

A macOS menu bar application for managing background processes like SSH tunnels, port forwarding, and development services. Everything is configured via a simple TOML file.

## Features

- Toggle any CLI tool on/off from the menu bar
- TOML based configuration
- Automatic process cleanup on app termination
- Custom PATH environment support
- Native macOS integration

## Installation

### Prerequisites

- Rust and Cargo (install via [rustup](https://rustup.rs/))
- Xcode Command Line Tools
- cargo-bundle (install with `cargo install cargo-bundle`)

### Building from Source

1. Clone the repository:
```bash
git clone https://github.com/vim-zz/something_bg.git
cd something_bg
```

2. Build and bundle the application:
```bash
cargo bundle --release
```

3. Move the app to your Applications folder:
```bash
cp -r "target/release/bundle/osx/Something in the Background.app" /Applications/
```

Launch the app from your Applications folder. It will appear as a menu bar item.

## Configuration

The app loads all menu items from `~/.config/something_bg/config.toml`. On first run, it creates this file with example configurations.

### Example Configuration

```toml
# Custom PATH for command execution
path = "/bin:/usr/bin:/usr/local/bin:/opt/homebrew/bin"

[tunnels]

# SSH tunnel with port forwarding
[tunnels.database]
name = "Database Tunnel"
command = "ssh"
args = ["-N", "-L", "5432:localhost:5432", "user@server.com"]
kill_command = "pkill"
kill_args = ["-f", "user@server.com"]

# Kubernetes port forwarding
[tunnels.k8s-service]
name = "K8s Service"
command = "kubectl"
args = ["port-forward", "svc/my-service", "8080:8080"]
kill_command = "pkill"
kill_args = ["-f", "svc/my-service"]

# Development server
[tunnels.dev-server]
name = "Dev Server"
command = "npm"
args = ["run", "dev"]
kill_command = "pkill"
kill_args = ["-f", "npm.*dev"]
```

Each tunnel needs:
- `name`: Display name in the menu
- `command` + `args`: Command to start the service
- `kill_command` + `kill_args`: Command to stop the service

Restart the app to pick up configuration changes.

## License

MIT
