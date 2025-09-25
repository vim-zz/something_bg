# Changelog

All notable changes to Something in the Background will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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