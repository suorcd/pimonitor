# Vim Keybindings Implementation

**Date:** 2026-01-09

## Summary
Added vim-style keyboard navigation to the pimonitor TUI application, enabled via a `--vim` command line flag.

## Changes Made

### 1. Command Line Argument Parsing
- Added argument parsing to detect `--vim` flag
- Parse arguments in `main()` function before initializing the application

### 2. AppState Structure
- Added `vim_mode: bool` field to `AppState` struct
- Modified `AppState::new()` to accept and store the vim_mode parameter

### 3. Vim Key Bindings
Implemented the following vim-style keybindings when `--vim` flag is enabled:

#### Navigation
- **j**: Move down (equivalent to Down arrow)
- **k**: Move up (equivalent to Up arrow)
- **h**: Move to beginning (equivalent to Home)
- **l**: Move to end (equivalent to End)
- **Ctrl-n**: Next item/scroll down (equivalent to Down arrow)
- **Ctrl-p**: Previous item/scroll up (equivalent to Up arrow)

#### Actions
- **Space**: Play the latest episode of the selected feed (equivalent to 'p' key)

### 4. Modal Scroll Support
All vim keybindings work correctly in modal contexts:
- XML viewer modal
- Feed details popup modal
- Main feed list

### 5. Conditional Activation
- Vim bindings only activate when `vim_mode` is true
- Standard keybindings (arrow keys, 'p', etc.) continue to work regardless of vim mode
- This allows users to choose their preferred navigation style

## Usage

Run the application with vim keybindings:
```bash
`cargo run -- --vim`
```

Run the application with standard keybindings:
```bash
cargo run
```

## Testing Recommendations
- Test all vim keybindings in the main feed list
- Test vim keybindings within the XML viewer modal
- Test vim keybindings within the feed details popup
- Verify that standard keybindings still work in both modes
- Test Ctrl-p and Ctrl-n combinations
