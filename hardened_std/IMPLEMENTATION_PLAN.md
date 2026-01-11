# hardened_std Implementation Plan

## Total Functions Needed: ~30

### Priority 1: Core Functions (19 functions)
These are critical for nvrc to compile and run.

#### fs module (8 functions)
1. ✅ `fs::write()` - Write with 100-byte limit and path validation
2. ✅ `fs::read_to_string()` - Read entire file to string
3. ✅ `fs::create_dir_all()` - Create directory recursively
4. ✅ `fs::remove_file()` - Delete file
5. ✅ `fs::read_link()` - Read symlink target
6. ✅ `fs::metadata()` - Get file metadata
7. ✅ `File` struct with `open()`
8. ✅ `OpenOptions` with `new()`, `write()`, `open()`

#### process module (4 functions/types)
1. ✅ `Command::new()` - Create command with &'static str path
2. ✅ `Command::args()`, `stdout()`, `stderr()`, `spawn()`, `status()`, `exec()`
3. ✅ `Child` with `try_wait()`, `wait()`, `kill()`
4. ✅ `Stdio::from()` - Convert File to Stdio
5. ✅ `ExitStatus::success()`

#### path module (3 functions)
1. ✅ `Path::new()` - Create path reference
2. ✅ `Path::exists()` - Check if path exists
3. ✅ `Path::is_symlink()` - Check if path is symlink

#### collections module (1 function)
1. ✅ `HashMap::from()` - Create from iterator
2. ✅ `HashMap::get()` - Get value by key

#### os::unix modules (3 functions)
1. ✅ `UnixDatagram::bind()`, `unbound()`, `send_to()`, `recv_from()`
2. ✅ `FileTypeExt` trait (is_fifo, is_char_device)
3. ✅ `MetadataExt` trait (mode)

### Priority 2: Sync/Threading (6 functions)
Lower priority - used in tests mostly.

1. `Once` - One-time initialization
2. `LazyLock` - Lazy static initialization
3. `Arc` - Reference counting
4. `AtomicBool` - Atomic boolean
5. `thread::sleep()` - Sleep duration
6. `Duration` - Time duration

### Priority 3: Other (5 functions)
Can be stubbed initially.

1. `panic::set_hook()` - Panic handler
2. `panic::catch_unwind()` - Catch panics
3. `env::var()` - Environment variables
4. `io::Write` trait - Writing to files
5. `os::fd::AsFd` - File descriptor trait

---

## nvrc Code Refactoring Needed

### Issue: Dynamic `format!()` in Error Contexts

Current nvrc code uses dynamic formatting in error contexts:

```rust
// execute.rs:21
.context(format!("failed to execute {command}"))?

// execute.rs:40
.with_context(|| format!("Failed to start {}", command))

// execute.rs:24
Err(anyhow!("{} failed with status: {}", command, status))
```

**Problem**: hardened_std only supports static strings in `.with_context()`:
```rust
impl Error {
    pub fn with_context(self, msg: &'static str) -> Self
}
```

### Solution Options:

#### Option 1: Use Static Error Messages (Recommended)
Replace dynamic messages with static ones:

```rust
// Before:
.context(format!("failed to execute {command}"))?

// After:
.with_context("failed to execute command")?
```

**Files to modify:**
- `src/execute.rs` - Lines 21, 24, 40
- `src/daemon.rs` - Check for similar patterns
- `src/kernel_params.rs` - Check for similar patterns
- `src/lockdown.rs` - Check for similar patterns
- `src/kmsg.rs` - Check for similar patterns

#### Option 2: Extend Error Type with Context Field
Store command name in error variant:

```rust
pub enum Error {
    CommandFailed { command: &'static str, status: i32 },
    CommandNotFound { command: &'static str },
    // ...
}
```

**Tradeoff**: More complex error handling, but preserves context.

#### Option 3: Hybrid Approach
Use static messages for most cases, but add specific error variants for critical paths:

```rust
// For execute.rs specifically:
pub enum Error {
    // ... other variants
    CommandExecuteFailed,
    CommandSpawnFailed,
}

// In execute.rs:
if !status.success() {
    return Err(Error::CommandExecuteFailed.into());
}
```

### Recommended: Option 1 (Static Messages)

**Rationale:**
- Simplest to implement
- Maintains security (no dynamic string allocation)
- Kernel logs (kmsg) already capture command output
- Stack traces show call site, which identifies the command

**Impact:** ~5-10 lines need modification in execute.rs and potentially daemon.rs

---

## Implementation Steps

### Step 1: Implement Core fs Module (8 functions)
Start with the most-used module.

**Functions to implement:**
1. `fs::write()` - Security: 100-byte limit, path whitelist
2. `fs::read_to_string()` - Security: path whitelist
3. `fs::create_dir_all()` - Use `libc::mkdir()` recursively
4. `fs::remove_file()` - Use `libc::unlink()`
5. `fs::read_link()` - Use `libc::readlink()`
6. `fs::metadata()` - Use `libc::stat()`
7. `File::open()` - Use `libc::open()`
8. `OpenOptions` - Builder for file opening

**Testing:**
```bash
cd hardened_std
cargo test fs::
```

### Step 2: Implement process Module (4 functions)
Critical for command execution.

**Functions to implement:**
1. `Command::new()` - Validate binary path against whitelist
2. `Command` methods - Store args, setup stdio
3. `Command::spawn()` - `fork()` + `execv()`
4. `Command::status()` - `spawn()` + `waitpid()`
5. `Child::try_wait()` - `waitpid()` with WNOHANG
6. `Stdio::from()` - Extract fd from File

**Testing:**
```bash
cargo test process::
```

### Step 3: Implement path Module (3 functions)
Simple wrapper around string operations.

**Functions to implement:**
1. `Path::new()` - Transmute &str to &Path
2. `Path::exists()` - `libc::access()` with F_OK
3. `Path::is_symlink()` - `libc::lstat()` and check S_IFLNK

**Testing:**
```bash
cargo test path::
```

### Step 4: Implement collections::HashMap (2 functions)
Fixed-size stack-allocated hashmap.

**Approach:**
```rust
pub struct HashMap<K, V> {
    entries: heapless::Vec<(K, V), 4>, // Stack-allocated, max 4 entries
}
```

Or simpler:
```rust
pub struct HashMap<K, V> {
    entries: [(Option<K>, Option<V>); 4],
    len: usize,
}
```

### Step 5: Implement os::unix modules (3 functions)
Unix-specific functionality.

**Functions to implement:**
1. `UnixDatagram` - Wrap socket fd
2. `FileTypeExt` trait - Already implemented (delegates to FileType)
3. `MetadataExt` trait - Already implemented (delegates to Metadata)

### Step 6: Update nvrc to Use hardened_std

1. **Update Cargo.toml**:
```toml
[workspace]
members = [".", "hardened_std"]

[dependencies]
hardened_std = { path = "./hardened_std" }
# Remove: anyhow
```

2. **Refactor error messages** (execute.rs, daemon.rs):
   - Replace `format!()` with static strings
   - Remove `.with_context(|| format!(...))` closures

3. **Update imports** in all 16 src files:
```rust
// Before:
use std::fs;
use anyhow::Result;

// After:
use hardened_std::fs;
use hardened_std::Result;
```

4. **Test compilation**:
```bash
cargo build --release
```

### Step 7: Implement Sync/Threading (if needed)
Only if tests fail.

### Step 8: Measure Results

```bash
# Before
ls -lh target/x86_64-unknown-linux-musl/release/nvrc

# After
ls -lh target/x86_64-unknown-linux-musl/release/nvrc

# Calculate reduction
```

---

## Security Constraints Summary

### fs::write()
- **Max size**: 100 bytes
- **Allowed paths**: `/proc/sys/*`, `/dev/*`, `/var/run/*`, `/tmp/*`
- **Violation**: Returns `Error::WriteTooLarge` or `Error::PathNotAllowed`

### Command::new()
- **Path type**: `&'static str` only (compile-time constants)
- **Allowed binaries**:
  - `/bin/nvidia-persistenced`
  - `/bin/nv-hostengine`
  - `/bin/dcgm-exporter`
  - `/bin/nv-fabricmanager`
  - `/usr/bin/nvidia-smi`
  - `/usr/bin/nvidia-ctk`
  - `/sbin/modprobe`
  - `/usr/bin/kata-agent`
  - Test binaries: `/bin/true`, `/bin/false`, `/bin/sh`, `/bin/sleep`
- **Violation**: Panics on invalid binary at construction time

### Command::args()
- **Arg type**: `&[&'static str]` only (compile-time constants)
- **Violation**: Type system enforces at compile time

### fs::read_to_string()
- **Allowed paths**: Same as `fs::write()`
- **Violation**: Returns `Error::PathNotAllowed`

---

## Current Status

✅ **Completed:**
- Directory structure created
- Cargo.toml configured
- Skeleton modules created (all compile with warnings)
- Error types defined
- Module structure established

📋 **Next Steps:**
1. Implement `fs::write()` with security constraints
2. Implement `fs::read_to_string()`
3. Continue with remaining fs functions
4. Implement process module
5. Refactor nvrc error messages
6. Update nvrc imports
7. Test and measure

---

## Testing Strategy

### Unit Tests (in hardened_std)
Test security constraints:

```rust
#[test]
fn test_write_size_limit() {
    let buf = [0u8; 101];
    assert!(matches!(
        fs::write("/proc/sys/test", &buf),
        Err(Error::WriteTooLarge(101))
    ));
}

#[test]
fn test_path_not_allowed() {
    assert!(matches!(
        fs::write("/etc/passwd", b"pwned"),
        Err(Error::PathNotAllowed)
    ));
}

#[test]
fn test_binary_not_allowed() {
    let result = std::panic::catch_unwind(|| {
        Command::new("/usr/bin/evil");
    });
    assert!(result.is_err());
}
```

### Integration Tests (nvrc)
Ensure existing tests pass:

```bash
cargo test  # NOT cargo test --lib!
```

---

## Success Metrics

- [ ] hardened_std compiles without errors
- [ ] All 19 Priority 1 functions implemented
- [ ] Security constraints enforced (unit tests pass)
- [ ] nvrc compiles with hardened_std
- [ ] nvrc tests pass
- [ ] Binary size reduced by ≥20%
- [ ] Attack surface measurably reduced (fewer symbols, smaller binary)
