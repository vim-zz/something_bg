# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Something in the Background is a macOS menu bar application written in Rust that manages SSH tunnels, Kubernetes port forwarding, and other background processes. It provides a simple interface for toggling various services through the system menu bar.

## Build and Development Commands

### Building the Application
```bash
cargo build --release
```

### Creating macOS App Bundle
```bash
cargo bundle --release
```
The app bundle will be created at `target/release/bundle/osx/Something in the Background.app`

### Installing to Applications
```bash
cp -r "target/release/bundle/osx/Something in the Background.app" /Applications/
```

### Running in Development
```bash
cargo run
```

## Architecture

### Core Components

- **main.rs** - Entry point with Cocoa setup, global app initialization, and run loop
- **app.rs** - Main application state containing TunnelManager and StatusItem wrapper
- **config.rs** - TOML configuration loading and management
- **menu.rs** - NSStatusItem and NSMenu creation, icon management, Objective-C bridge
- **tunnel.rs** - SSH tunnel and port forwarding logic with TunnelManager
- **logger.rs** - macOS-specific logging configuration using oslog

### Key Design Patterns

1. **Global App State**: Uses `OnceLock<App>` for thread-safe global access from Objective-C callbacks
2. **Thread-Safe Wrappers**: `StatusItemWrapper` makes Cocoa objects thread-safe
3. **Objective-C Bridge**: Custom MenuHandler class bridges Cocoa events to Rust functions
4. **Async Tunnel Management**: Spawns threads for long-running SSH processes

### Tunnel Configuration

Tunnels are configured via TOML file at `~/.config/something_bg/config.toml`. The configuration file is automatically created with defaults on first run. Default tunnels include:
- **example-ssh**: SSH tunnel with port forwarding
- **k8s-example**: Kubernetes port forwarding for services
- **colima**: Docker environment management via Colima

Each tunnel configuration includes:
- `name`: Display name in the menu
- `command`: Command to execute
- `args`: Arguments for the command
- `kill_command`: Command to stop the tunnel
- `kill_args`: Arguments for the kill command
- `separator_after` (optional): Boolean to add a separator line after this menu item
- `group_header` (optional): String to display as a non-clickable section header before this item
- `group_icon` (optional): SF Symbol name to display with the group header (e.g., "sf:cylinder.fill")

The configuration also includes a global `path` setting that defines the PATH environment variable used when executing commands. This allows customization of where the system looks for executables.

The configuration structure in `config.rs` handles TOML serialization/deserialization and loading from the user's home directory.

### Configuration Management

The app uses a TOML-based configuration system:

1. **Default Configuration**: If no config file exists, the app creates one with default tunnels
2. **Dynamic Loading**: Configuration is loaded both at startup and when creating the menu
3. **Fallback Behavior**: If config loading fails, the app falls back to hardcoded defaults
4. **User Customization**: Users can modify `~/.config/something_bg/config.toml` to add/remove tunnels

The configuration format supports any command-line tool that can be started and stopped, not just SSH tunnels.

### Menu Organization Features

The app supports organizing menu items with visual enhancements:

1. **Group Headers**: Non-clickable section headers with SF Symbol icons
   - Implemented in `menu.rs` via `create_header_item()` function
   - Uses `NSMenuItem` with `setEnabled(false)` for disabled state
   - Loads SF Symbols via `imageWithSystemSymbolName_accessibilityDescription()`
   - Headers are optional per-item (not per-section), allowing flexible grouping
   - Built with objc2 for type-safe Objective-C interop

2. **Menu Separators**: Visual dividers between menu items
   - Implemented using `NSMenuItem::separatorItem()`
   - Controlled by `separator_after` field in tunnel config
   - Added automatically after items where `separator_after = true`

3. **Icon Loading**: Supports SF Symbols for group headers
   - Icon loader function `load_icon()` handles SF Symbol format: "sf:symbol.name"
   - Falls back gracefully if symbol fails to load
   - Icons are sized to 16x16 for menu items
   - Uses objc2's type-safe `Retained<NSImage>` for memory management

### Icon System

The app uses PNG icons in `resources/images/` with two states:
- **Inactive**: `peacock_folded_16x16.png`
- **Active**: `peacock_open_16x16.png`

Falls back to Unicode symbols (☷/☰) if images fail to load.

## Dependencies

- **cocoa/objc**: macOS Cocoa framework bindings
- **core-foundation**: Core Foundation framework access
- **log/oslog**: macOS system logging
- **libc**: System calls
- **toml**: TOML configuration file parsing
- **serde**: Serialization/deserialization framework

Uses a patched version of `objc` crate from a third-party fork for compatibility.

## Important Notes

- App uses NSApplicationActivationPolicyAccessory (background/menu bar only)
- Tunnels are cleaned up automatically on app termination
- Status item icon updates based on active tunnel state
- PATH is extended to include Homebrew binaries for command execution
- Uses objc2 for type-safe Objective-C interop (migrated from legacy objc crate)
- All optional fields (`separator_after`, `group_header`, `group_icon`) use `#[serde(default)]` for backward compatibility
- TOML special characters (like backslash) must be escaped with `\\` in configuration strings