# Release Notes - Something in the Background v1.0.2

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