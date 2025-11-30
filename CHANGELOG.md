# Changelog

All notable changes to Something in the Background will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.4.1] - 2025-11-30

### Changed
- Scheduled task "Next run" and "Last run" timestamps now display in human-friendly relative terms (e.g., "tomorrow at 10:00") and refresh each time the menu opens.

## [1.3.2] - 2025-11-17

### Changed
- **Code Architecture**: Refactored About window into its own dedicated module (`about.rs`)
  - Improved separation of concerns with modular code organization
  - Window logic now isolated from menu handling code
  - Better maintainability with focused, single-responsibility modules

### Fixed
- **Memory Leak**: Eliminated memory leaks in About window implementation
  - Replaced `std::mem::forget()` with proper lifecycle management using `thread_local!` storage
  - Window and button helper objects now properly managed instead of leaked
  - Added window reuse - opening About again brings existing window to front instead of creating duplicates

### Technical Details
- **Unsafe Code Minimization**: Reduced scope of unsafe blocks across the codebase
  - Removed dangerous `std::mem::transmute` calls, replaced with safer pointer casts
  - Created reusable helper functions that encapsulate unsafe operations:
    - `create_menu_item_with_action()` - wraps unsafe menu item creation
    - `set_menu_item_target()` - wraps unsafe setTarget calls
    - `set_menu_item_represented_object()` - wraps unsafe setRepresentedObject
    - `extract_nsstring_from_object()` - centralized NSString extraction with documented safety invariants
  - Unsafe blocks reduced from 100+ line scopes to focused single-line operations
  - Added comprehensive SAFETY comments documenting invariants for each unsafe operation
- **Thread-Safe Storage**: About window state stored in `thread_local! { RefCell<Option<AboutWindowState>> }`
  - Properly handles main-thread-only Cocoa objects
  - Prevents window deallocation while maintaining ability to clean up
- **Code Quality**: Improved idiomatic Rust patterns
  - Better memory safety through proper RAII patterns
  - Cleaner, more maintainable codebase with smaller focused functions
  - Enhanced compile-time safety with reduced unsafe surface area

## [1.3.0] - 2025-11-16

### Added
- **About Window**: New floating About dialog accessible from the menu
  - Displays app version number dynamically
  - Shows circle ring icon matching the new app branding
  - Includes clickable "View on GitHub" button that opens the repository in browser
  - Shows copyright information (© 2025 Ofer Affias)
  - Window floats above other windows and activates the app when opened
  - Clean, titleless window design

### Changed
- **App Icon**: Replaced rocket icon with minimalist circle ring design
  - New `circle.icns` icon generated at all standard macOS sizes (16x16 to 1024x1024)
  - Matches the status bar icon style (○ for inactive, ● for active)
  - Consistent branding across app icon and status bar
- **About Menu Item**: Changed from disabled informational text to clickable action
- **Menu Item Appearance**: Removed automatic SF Symbol icons from About and Quit menu items
  - Uses custom action selectors (`displayAppInfo:`, `exitApplication:`) to prevent macOS from adding default symbols
  - Cleaner, more consistent menu appearance without system-imposed icons

### Technical Details
- Added AppKit window support: `NSWindow`, `NSTextField`, `NSImageView`, `NSButton`
- Implemented `URLButtonHelper` class for handling GitHub link button clicks
- Created custom `displayAppInfo:` selector (avoiding "About" keyword in selector name)
- Created custom `exitApplication:` selector (avoiding `terminate:` standard selector)
- These custom selectors prevent macOS Big Sur+ automatic SF Symbol assignment
- Added `.venv/` to `.gitignore` for Python virtual environment (used in icon generation)
- Icon generation script creates proper .icns from circle ring PNGs

## [1.2.0] - 2025-01-13

### Added
- **Scheduled Tasks** - Run commands automatically based on cron schedules
  - New `[schedules]` configuration section for defining periodic tasks
  - Full cron syntax support (minute, hour, day, month, weekday)
  - Human-readable schedule descriptions in menu (e.g., "Every day at 6:00")
  - Last run timestamps displayed in local timezone with 24-hour format
  - "Run Now" button in submenu to manually trigger tasks
  - Automatic background execution - checks every 30 seconds for due tasks
  - Real-time menu updates showing current task status
- New dependencies: `croner` (cron parsing), `chrono` (datetime), `serde_json` (state management)
- Scheduled tasks display in submenus with:
  - Schedule information (grayed out, informational)
  - Last run time (grayed out, auto-updating)
  - Manual "Run Now" action button

### Changed
- Menu items now update automatically when opened via `NSMenuDelegate` implementation
- Task state persisted in-memory with thread-safe access
- Scheduler runs in background thread, automatically starts on app launch

### Technical Details
- New `scheduler.rs` module with `TaskScheduler` and `ScheduledTask` types
- Uses `croner` v2.0 for robust cron expression parsing and scheduling
- Integrates `chrono` for timezone-aware datetime handling
- Background scheduler thread checks tasks every 30 seconds
- Graceful cleanup - scheduler stops when app terminates
- Menu delegate pattern for dynamic content updates without full menu rebuild

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
