# Changelog

All notable changes to Something in the Background will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0] - 2025-01-12

### Added
- "Open Config Folder" menu item to quickly access configuration file

## [1.0.4] - 2025-01-11

### Added
- Optional group headers with SF Symbol icons for organizing menu items (`group_header`, `group_icon`)
- Optional separators between menu items (`separator_after`)
- All fields are backward compatible - existing configs continue to work unchanged

## [1.0.3] - 2025-11-10

### Changed
- **Major internal modernization**: Complete migration from deprecated `objc` and `cocoa` crates to modern `objc2` ecosystem
  - Replaced manual Objective-C bindings with `objc2`, `objc2-foundation`, and `objc2-app-kit` crates
  - Updated to Rust 2024 Edition for latest language features
  - **Zero breaking changes**: All functionality remains identical for end users
  - Same configuration format, file locations, and behavior

### Technical Details
- **Memory Management**: Migrated from raw `id` pointers to `Retained<T>` smart pointers for automatic reference counting
- **Class Declaration**: Replaced `ClassDecl::new()` + `add_method()` with modern `define_class!` macro
- **Type Safety**: Enhanced compile-time guarantees with stronger Rust-Objective-C type integration
- **API Updates**:
  - `NSString::alloc(nil).init_str()` → `NSString::from_str()`
  - Manual memory management → Automatic with `Retained<T>`
  - `NO`/`YES` constants → `0`/`1` integers
  - Removed `NSAutoreleasePool` (managed automatically by objc2)
- **Dependencies**: Removed patched `objc` fork dependency, now using stable crates from crates.io
- **Binary Size**: Optimized release build to 623 KB with modern compiler optimizations

### Benefits
- Better memory safety with automatic reference counting preventing memory leaks
- Modern APIs with up-to-date bindings for latest macOS SDKs (Xcode 16.4)
- Active maintenance: `objc2` is actively developed with 100% API documentation coverage
- Future-proof foundation for supporting newer macOS features
- Improved developer experience with better type safety and error messages

## [1.0.2] - 2025-01-27

### Fixed
- **Menu item ordering**: Menu items now appear in the same order as defined in the configuration file
  - Previously, menu items were displayed in a random order due to HashMap's non-deterministic iteration
  - Now preserves the exact order from `config.toml` using TOML's `preserve_order` feature and Vec data structure
  - No additional dependencies required - uses existing TOML crate with `preserve_order` feature flag

### Changed
- Internal data structure changed from `HashMap<String, TunnelConfig>` to `Vec<(String, TunnelConfig)>` for tunnels configuration
- Enhanced TOML parsing to maintain insertion order during deserialization
- Added custom serialization helper to maintain TOML file structure compatibility

### Technical Details
- Enabled `preserve_order` feature in toml crate dependency
- Implemented custom `from_toml_value()` method for order-preserving configuration loading
- Added `ConfigForSerialization` helper struct for proper TOML output formatting

## [1.0.1] - 2025-01-27

### Added
- Initial release of Something in the Background
- SSH tunnel management through system menu bar
- Kubernetes port forwarding support
- TOML-based configuration system
- macOS status bar integration with visual indicators
- Automatic cleanup of tunnels on app termination
- Configurable PATH environment for command execution
- Support for custom start/stop commands per tunnel

### Features
- **Menu Bar Integration**: Clean macOS menu bar app with peacock icons
- **Multiple Tunnel Types**: SSH tunnels, Kubernetes port forwarding, and custom commands
- **Visual Feedback**: Status icons change based on active tunnel state
- **Configuration File**: `~/.config/something_bg/config.toml` for easy customization
- **Process Management**: Reliable start/stop of background processes
- **Error Handling**: Graceful handling of failed commands with retry logic