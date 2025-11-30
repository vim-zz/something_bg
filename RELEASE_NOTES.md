# Release Notes - Something in the Background

## v1.4.1

**Release Date:** November 30, 2025

### ⏱️ Friendlier Scheduled Task Times

- Scheduled task "Next run" and "Last run" entries now show natural, relative phrasing like "tomorrow at 10:00" or "next Monday at 09:30".
- Menu entries refresh on every open, so times stay accurate to the moment you click the status bar.

## v1.3.2

**Release Date:** November 17, 2025

### 🛠️ Code Quality & Memory Safety Improvements

This release focuses on **internal code quality improvements** that make Something in the Background more robust, maintainable, and memory-safe. No user-facing changes, but significant improvements under the hood!

#### What's Fixed

**Memory Leak Eliminated**
- Fixed memory leak in About window that occurred every time the window was opened
- Window and helper objects are now properly managed instead of being leaked
- Bonus: Opening About again now brings the existing window to front instead of creating duplicates

#### What's Improved

**Better Code Architecture**
- Refactored About window into its own dedicated module (`about.rs`)
- Improved separation of concerns - window logic now isolated from menu handling
- More maintainable codebase with focused, single-responsibility modules

**Safer Rust Code**
- Removed dangerous `std::mem::transmute` calls throughout the codebase
- Minimized unsafe code blocks from 100+ line scopes to focused single-line operations
- Created reusable helper functions that encapsulate unsafe Objective-C interop
- Added comprehensive safety documentation for all remaining unsafe operations
- Better memory safety through proper RAII patterns

#### Technical Highlights

**Unsafe Code Minimization**
- `create_menu_item_with_action()` - wraps unsafe NSMenuItem creation
- `set_menu_item_target()` - wraps unsafe setTarget calls
- `set_menu_item_represented_object()` - wraps unsafe setRepresentedObject
- `extract_nsstring_from_object()` - centralized NSString extraction with safety invariants

**Thread-Safe Window Management**
```rust
// Before: Memory leak
std::mem::forget(window);
std::mem::forget(url_helper);

// After: Proper lifecycle management
thread_local! {
    static ABOUT_WINDOW: RefCell<Option<AboutWindowState>> = RefCell::new(None);
}
```

**Why This Matters**
- Prevents memory leaks that would accumulate over time
- Reduces potential for undefined behavior from unsafe code
- Makes future development safer and easier
- Professional code quality following Rust best practices

---

## v1.3.0

**Release Date:** November 16, 2025

### 🎨 New About Window & Refreshed Branding

We're excited to introduce a **new About window** and **refreshed app icon** that brings a cleaner, more consistent look to Something in the Background!

#### What's New

**About Window**
- Click "About" in the menu to open a floating information window
- Displays version number, copyright info, and a clickable GitHub link
- Features the new circle ring icon as the app logo
- Window floats above other apps for easy visibility
- Clean, minimal design with no title bar clutter

**New App Icon**
- Replaced the rocket icon with a minimalist **circle ring** design
- Matches the status bar indicators (○ inactive / ● active)
- Consistent branding throughout the app
- Professional, understated aesthetic

**Cleaner Menus**
- Removed automatic SF Symbol icons from About and Quit menu items
- macOS Big Sur+ was adding these icons automatically based on action names
- Custom selectors now prevent system-imposed decorations
- Cleaner, more intentional menu appearance

#### Technical Highlights

- **Window Architecture**: Full NSWindow implementation with NSTextField, NSImageView, NSButton
- **URL Handler**: Clickable "View on GitHub" button opens browser directly
- **Icon Generation**: Automated script creates .icns at all standard macOS sizes
- **macOS Compatibility**: Works around Big Sur's automatic SF Symbol assignment

---

## v1.2.0

**Release Date:** January 13, 2025

### 🎯 Scheduled Tasks - Automate Your Workflows

We're excited to introduce **Scheduled Tasks** - the ability to run commands automatically based on cron schedules directly from your menu bar!

#### What's New

**Cron-Based Scheduling**
- Schedule any command to run periodically using standard cron syntax
- Perfect for backups, health checks, data syncs, and maintenance tasks
- Runs in the background - no need to keep terminal windows open

**Smart Menu Integration**
- Each scheduled task appears with a submenu showing:
  - **Schedule**: Human-readable description (e.g., "Every day at 6:00")
  - **Last run**: Timestamp in your local timezone (24-hour format)
  - **Run Now**: Button to manually trigger the task immediately
- Real-time updates - last run times refresh automatically when you open the menu

**Easy Configuration**
Simply add a `[schedules]` section to your `config.toml`:

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
```

#### Common Cron Examples

- `0 * * * *` - Every hour
- `*/15 * * * *` - Every 15 minutes
- `0 6 * * *` - Every day at 6:00 AM
- `0 9 * * 1` - Every Monday at 9:00 AM
- `0 0 1 * *` - First day of every month at midnight

#### Use Cases

**Development Workflows**
- Hourly database backups during development
- Periodic cache clearing
- Scheduled test runs

**System Maintenance**
- Daily log rotation
- Weekly cleanup scripts
- Periodic health checks

**Data Operations**
- Hourly data syncs
- Scheduled report generation
- Periodic API polling

#### Technical Highlights

- **Robust Parsing**: Uses `croner` library for reliable cron expression handling
- **Timezone-Aware**: All timestamps displayed in your local timezone
- **Thread-Safe**: Background scheduler runs independently without blocking UI
- **Automatic Cleanup**: Scheduler stops gracefully when app quits
- **Efficient**: Checks for due tasks every 30 seconds

### 🔄 What's Changed

- Menu items now update dynamically when opened (via `NSMenuDelegate`)
- New dependencies: `croner`, `chrono`, `serde_json`
- Enhanced menu system with submenu support for scheduled tasks

### 🚀 Upgrade Instructions

#### For Existing Users

1. **Download** the new version from the release assets
2. **Replace** the existing app:
   ```bash
   cp -r "Something in the Background.app" /Applications/
   ```
3. **Update your config** (optional) - add `[schedules]` section for any tasks you want automated
4. **Restart** the app - scheduler starts automatically!

#### Building from Source

```bash
git pull origin main
cargo bundle --release
cp -r "target/release/bundle/osx/Something in the Background.app" /Applications/
```

### 📋 Example Configuration

Here's a complete example combining tunnels and scheduled tasks:

```toml
path = "/bin:/usr/bin:/usr/local/bin:/opt/homebrew/bin"

# Tunnels (toggle on/off)
[tunnels.database-prod]
name = "PROD Database"
command = "ssh"
args = ["-N", "-L", "5432:localhost:5432", "user@prod.example.com"]
kill_command = "pkill"
kill_args = ["-f", "user@prod.example.com"]
group_header = "DATABASE"
group_icon = "sf:cylinder.fill"
separator_after = true

# Scheduled Tasks (automatic execution)
[schedules.daily-backup]
name = "Daily Backup"
command = "/usr/local/bin/backup.sh"
args = ["--compress"]
cron_schedule = "0 6 * * *"
group_header = "SCHEDULED TASKS"
group_icon = "sf:clock.fill"

[schedules.health-check]
name = "API Health Check"
command = "curl"
args = ["-f", "https://api.example.com/health"]
cron_schedule = "*/30 * * * *"
```

### 🐛 Bug Reports

If you encounter any issues:

1. Check you're using v1.2.0
2. Verify your cron syntax at [crontab.guru](https://crontab.guru/)
3. Check logs with `log show --predicate 'subsystem == "com.vim-zz.something-bg"' --last 1h`
4. Report issues with:
   - Your macOS version
   - Sample `config.toml` (sanitized)
   - Steps to reproduce

### 💡 What's Next

Future releases may include:
- Visual cron schedule builder
- Task execution history
- Email/notification on task completion
- Task dependencies and chains
- Export/import of scheduled task sets

---

**Full Changelog:** [CHANGELOG.md](CHANGELOG.md)

**Download:** See release assets below

**System Requirements:** macOS 10.15+ (Catalina or later)

## v1.0.3

**Release Date:** November 10, 2025

### 🔄 Major Internal Modernization

**Migration to objc2:** Complete rewrite of Objective-C bindings using the modern `objc2` crate ecosystem.

#### What Changed

**Under the Hood:**
- Migrated from deprecated `objc` and `cocoa` crates to modern `objc2`, `objc2-foundation`, and `objc2-app-kit`
- Replaced manual memory management with automatic reference counting via `Retained<T>`
- Updated class declaration from `ClassDecl` to modern `define_class!` macro
- Enhanced type safety with stronger compile-time guarantees
- Improved memory safety and reduced potential for leaks

**For Users:**
- ✅ **Zero breaking changes** - all functionality remains identical
- ✅ Same configuration format and file locations
- ✅ Same menu bar interface and behavior
- ✅ All tunnel commands work exactly as before

#### Technical Benefits

1. **Better Memory Safety**: Automatic reference counting prevents memory leaks
2. **Modern APIs**: Up-to-date bindings for latest macOS SDKs (Xcode 16.4)
3. **Active Maintenance**: `objc2` is actively developed with 100% API documentation
4. **Future-Proof**: Foundation for supporting newer macOS features
5. **Smaller Binary**: Optimized release build (623 KB)

---

## v1.0.2

**Release Date:** January 27, 2025

## 🎯 What's Fixed

### Menu Item Ordering Issue Resolved

**Problem:** Menu items in the status bar were appearing in random order, making it difficult to find specific tunnels consistently.

**Solution:** Menu items now appear in the exact same order as they're defined in your `config.toml` file!

**Before:**
```
❌ Random order every time:
├── Open tunnel PROD
├── Langfuse port forward  
├── Open tunnel DEV-01
└── Colima Docker
```

**After:**
```
✅ Consistent order matching config.toml:
├── Open tunnel PROD          (from [tunnels.prod])
├── Open tunnel DEV-01        (from [tunnels.dev-01]) 
├── Langfuse port forward     (from [tunnels.k8s-langfuse])
└── Colima Docker             (from [tunnels.colima])
```

## 🔧 Technical Implementation

- **No Breaking Changes**: Your existing `config.toml` files continue to work exactly as before
- **Zero Additional Dependencies**: Used existing TOML library's `preserve_order` feature
- **Internal Optimization**: Changed from `HashMap` to `Vec` data structure for natural ordering
- **Smart Serialization**: Maintains proper TOML file format when saving configurations

## 🚀 Upgrade Instructions

### For Existing Users

1. **Download** the new version from the release assets
2. **Replace** the existing app:
   ```bash
   cp -r "Something in the Background.app" /Applications/
   ```
3. **Restart** the app - your configuration will automatically work with proper ordering!

### Building from Source

```bash
git pull origin main
cargo bundle --release
cp -r "target/release/bundle/osx/Something in the Background.app" /Applications/
```

## 📁 Configuration File Structure

Your `~/.config/something_bg/config.toml` order is now preserved:

```toml
[tunnels.first-tunnel]      # ← Will appear first in menu
name = "First Tunnel"

[tunnels.second-tunnel]     # ← Will appear second in menu  
name = "Second Tunnel"

[tunnels.third-tunnel]      # ← Will appear third in menu
name = "Third Tunnel"
```

## 🐛 Bug Reports

If you encounter any issues with this release, please:

1. Check that you're using the latest version (v1.0.2)
2. Verify your `config.toml` file is properly formatted
3. Report issues with:
   - Your macOS version
   - Contents of your `config.toml` 
   - Steps to reproduce the problem

## 💡 What's Next

Future releases will focus on:
- Additional tunnel types and protocols
- Enhanced error reporting and logging
- GUI configuration editor
- Export/import of tunnel configurations

---

**Full Changelog:** [CHANGELOG.md](CHANGELOG.md)

**Download:** See release assets below

**System Requirements:** macOS 10.15+ (Catalina or later)
