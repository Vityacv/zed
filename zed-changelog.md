# Zed Build Changelog

## 2025-10-19 - REPL/Jupyter Dependency Issue

### Issue
Build failed with runtime panic when starting Zed:
```
thread 'main' panicked at crates/zed/src/zed.rs:1609:87:
called `Result::unwrap()` on an `Err` value: Error loading built-in keymap "keymaps/default-linux.json"
In binding `"ctrl-shift-enter"`, didn't find an action named `"repl::Run"`.
```

### Root Cause
The `jupyter-websocket-client` dependency (from the `repl` crate) requires `async-tungstenite` 0.29.1 with the `tokio-runtime` feature, but this feature was not specified in the dependency declaration. This caused compilation errors:

```
error[E0432]: unresolved import `async_tungstenite::tokio`
  --> jupyter-websocket-client/src/client.rs:3:5
   |
 3 |     tokio::connect_async,
   |     ^^^^^ could not find `tokio` in `async_tungstenite`
```

### Attempted Solutions
1. Added `tokio-runtime` feature to workspace `async-tungstenite` dependency - failed due to version mismatch
2. Tried patching with git repository - Cargo doesn't allow patching to same source
3. Attempted adding features in patch section - Cargo doesn't support features in patches

### Applied Workaround
Temporarily disabled REPL/Jupyter functionality:

1. **Commented out repl crate** in `/data/projects/zed/Cargo.toml`:
   - Line 139: Commented out `"crates/repl"` from workspace members
   - Lines 530-531, 544: Commented out jupyter dependencies

2. **Removed repl dependency** in `/data/projects/zed/crates/zed/Cargo.toml`:
   - Line 119: Commented out `repl.workspace = true`

3. **Disabled repl initialization** in `/data/projects/zed/crates/zed/src/main.rs`:
   - Lines 585, 592: Commented out `repl::init()` and `repl::notebook::init()`

4. **Removed repl menu** in `/data/projects/zed/crates/zed/src/zed/quick_action_bar.rs`:
   - Line 2: Commented out `mod repl_menu`
   - Line 601: Commented out `.children(self.render_repl_menu(cx))`

5. **Removed jupyter keybindings** in `/data/projects/zed/assets/keymaps/default-linux.json`:
   - Deleted lines 179-185 containing the jupyter context keybindings for `repl::Run` and `repl::RunInPlace`

### Result
- Build completes successfully in release mode
- Zed starts and runs normally without REPL functionality
- All jupyter-related keybindings removed from default keymap

### Future Resolution
**TODO: Re-enable REPL functionality when upstream dependency is fixed**

This is a temporary workaround. To re-enable REPL functionality:
1. The upstream `jupyter-websocket-client` crate needs to properly specify the `tokio-runtime` feature for `async-tungstenite` 0.29.1
2. Alternatively, update to a newer version of `async-tungstenite` that doesn't require this feature flag
3. Once fixed upstream, uncomment all the changes listed above and rebuild

**Note:** REPL was disabled as a workaround, but we want to re-enable it once the dependency issue is resolved. The feature has zero runtime overhead when not in use and may be useful in the future.
