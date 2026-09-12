# Platform Loading Strategy: Zero-Copy In-Memory

## Overview

Roc applications use **platforms** to access system I/O and other services. The hello_world uses:
```roc
app [main!] { pf: platform "https://github.com/roc-lang/basic-cli/releases/download/0.20.0/X73hGh05nNTkDHU06FHC0YfFaQB1pimX7gncRcao5mU.tar.br" }
```

This document describes how we'll efficiently load and use platforms without copying data.

---

## Platform Structure (Downloaded Example)

### File: basic-cli.tar.br (14MB)
```
Brotli-compressed TAR archive containing:

Binary files (compiled platform code for each architecture):
├── linux-arm64.a         (compiled for ARM64 Linux)
├── linux-x64.a           (compiled for x86-64 Linux)
├── macos-arm64.a         (compiled for ARM64 macOS)
└── macos-x64.a           (compiled for x86-64 macOS)

Metadata:
├── metadata_linux-x64.rm (machine-readable metadata)
└── linux-x64.rh          (Roc header)

Module definitions (Roc source):
├── main.roc              (entry point)
├── Stdout.roc            (standard output - used by hello_world)
├── Stdin.roc             (standard input)
├── File.roc              (file I/O)
├── Env.roc               (environment variables)
├── Http.roc              (HTTP client)
├── Cmd.roc               (command execution)
├── Tcp.roc               (TCP sockets)
├── Host.roc              (internal: connects Roc to platform binaries)
├── InternalIOErr.roc     (error handling)
└── [20+ more modules]
```

### What We Extract (Roc Modules Only)

For our v1.0 interpreter, we focus on **Roc module definitions**, not binary compilation:

**Stdout.roc (simplified):**
```roc
module [IOErr, line!, write!, write_bytes!]

import Host
import InternalIOErr

IOErr : [NotFound, PermissionDenied, BrokenPipe, ...]

line! : Str => Result {} [StdoutErr IOErr]
line! = |str|
    Host.stdout_line!(str)
    |> Result.map_err(handle_err)
```

**What we extract:**
- Module name: `"Stdout"`
- Exports: `["IOErr", "line!", "write!", "write_bytes!"]`
- Types: `IOErr` (tag union definition)
- Functions: `line!` with type `Str => Result {} [StdoutErr IOErr]`

---

## Zero-Copy Loading Strategy

### Step 1: Download Platform (Once, Cached)

```rust
// Download from URL
// Decompress: basic-cli.tar.br → basic-cli.tar (65MB uncompressed)
// Extract: tar → individual .roc files in memory

let platform_bytes = download_and_decompress(url)?;
let entries = tar::read_archive(platform_bytes)?;
```

**Cache location:**
```
~/.rocache/platforms/
└── <sha256(url)>/
    ├── manifest.json        (what was in archive)
    └── basic-cli.tar.br     (original download, never modified)
```

### Step 2: Parse Roc Modules into AST

```rust
// For each .roc file in the archive:
for entry in tar_entries {
    if entry.path.ends_with(".roc") {
        let roc_source = entry.read_string();
        
        // Parse using same nom-based parser as Phase 1
        let ast = parser.parse_module(&roc_source)?;
        
        // Store in arena (no copying of AST nodes)
        let module = platform.ast_arena.alloc(ast);
        
        modules.insert(entry.path, module);
    }
}
```

### Step 3: Extract Type Signatures

```rust
// From each module, extract what we need:
for (name, module_ast) in modules {
    for item in module.exports {
        match item {
            Expr::TypeDef(name, type_def) => {
                // Store type: "IOErr" → [NotFound, PermissionDenied, ...]
                platform.types.insert(name, type_def);
            }
            Expr::FunctionDef(name, type_sig) => {
                // Store function: "line!" → Str => Result {} [...]
                platform.functions.insert(name, type_sig);
            }
        }
    }
}
```

### Step 4: Global Static Storage

```rust
// All platforms cached in a global static
lazy_static::lazy_static! {
    static ref PLATFORM_CACHE: Mutex<HashMap<String, Platform>> = 
        Mutex::new(HashMap::new());
}

pub struct Platform {
    // Name: "pf" (from app declaration)
    pub name: String,
    
    // All modules from this platform
    pub modules: HashMap<&'static str, RocModule>,
    
    // Shared arena for all AST nodes (14MB for basic-cli)
    // NOT PER-MODULE: single arena for entire platform
    pub ast_arena: Rc<AstArena>,
    
    // All strings are interned references
    pub string_pool: Rc<StringPool>,
}

pub struct RocModule {
    // Module name: "Stdout"
    pub name: &'static str,
    
    // All exports use &'static str (interned)
    pub exports: HashMap<&'static str, ModuleItem>,
}

pub enum ModuleItem {
    Type { 
        name: &'static str,
        definition: Type,
    },
    Function {
        name: &'static str,
        type_sig: Type,  // Str => Result {} [StdoutErr IOErr]
    },
}
```

### Zero-Copy Guarantees

1. **Downloaded once** - Cached on disk, in memory only once per process
2. **Parsed once** - AST nodes allocated in arena
3. **String interning** - All identifiers are `&'static str` references, never copied
4. **Shared ownership** - Multiple modules can reference same types via `Rc<>`
5. **No cloning** - Only references passed around

**Memory layout:**
```
Platform "pf" (14MB total)
├── Archive bytes (14MB)
├── AST arena (reference)
│   ├── Stdout module
│   │   ├── line! function signature
│   │   └── IOErr type definition
│   ├── Stdin module
│   └── [other modules]
└── String pool (references)
    ├── "Stdout" → &'static str
    ├── "line!" → &'static str
    ├── "IOErr" → &'static str
    └── [all other identifiers]
```

---

## Using Imported Modules

### App Declaration Parsing

```roc
app [main!] { pf: platform "https://..." }
```

**Parser extracts:**
- Exports: `["main!"]`
- Platform binding: name=`"pf"`, url=`"https://..."`

**Type checker:**
1. Download and load platform (first use only)
2. Bind name `"pf"` to Platform
3. Make modules available for import

### Import Resolution

```roc
import pf.Stdout
```

**Parser extracts:**
- Platform ref: `"pf"`
- Module name: `"Stdout"`
- Items: all exports from Stdout (or specific list)

**Type checker:**
- Look up platform `"pf"` in cache
- Find module `"Stdout"` in platform
- Bind name `Stdout` to that module
- Make exports available: `line!`, `write!`, etc.

### Function Call Resolution

```roc
Stdout.line!("Hello")
```

**Parser:** Qualified function call
**Type checker:**
1. Look up module `Stdout`
2. Find function `line!` in module
3. Get type signature: `Str => Result {} [StdoutErr IOErr]`
4. Type-check argument and return

**Evaluator:**
1. Find platform function implementation
2. Call native Rust code (via Host module bridge)

---

## Dependencies Needed

Add to `Cargo.toml`:

```toml
[dependencies]
reqwest = { version = "0.11", features = ["stream"] }  # HTTP download
brotli = "3.3"                                          # Decompress .tar.br
tar = "0.4"                                             # Extract tar archive
sha2 = "0.10"                                           # Hash URLs for cache
```

---

## Implementation Phases

### Phase 16a: Platform Download & Caching
- [ ] Add HTTP client dependency
- [ ] Implement download_platform(url) → Vec<u8>
- [ ] Add ~/.rocache directory creation
- [ ] Cache by URL hash (SHA-256)
- [ ] Decompress brotli tar archives
- [ ] Store manifest.json

### Phase 16b: Platform Parsing
- [ ] Extract .roc files from tar
- [ ] Parse module definitions (reuse nom parser)
- [ ] Extract type signatures
- [ ] Store in platform cache

### Phase 16c: Import Resolution
- [ ] Parser: `import pf.Module`
- [ ] Type checker: resolve imports
- [ ] Environment: bind imported modules

### Phase 16d: Qualified Calls
- [ ] Parser: `Module.function(args)`
- [ ] Type checker: resolve to platform function
- [ ] Evaluator: execute native implementation

### Phase 16e: Host Bridge (v2.0)
- [ ] Implement Host module functions
- [ ] Execute effects (stdout, file I/O, etc.)
- [ ] Error handling

---

## Example: Using Stdout.line!

**Input file (hello_world/main.roc):**
```roc
app [main!] { pf: platform "https://...basic-cli...tar.br" }

import pf.Stdout

birds = -3

main! = |_args|
  Stdout.line!("There are ${Num.to_str(birds)} birds.")
```

**Processing:**

1. **App declaration parsed:**
   - Bind `pf` → Platform(url)
   - Download & parse platform (14MB → memory)

2. **Import resolved:**
   - `import pf.Stdout`
   - Look up `Stdout` module in platform cache
   - Bind `Stdout` → RocModule { exports: [line!, ...] }

3. **Variable binding:**
   - `birds = -3` → Value::Int(-3)

4. **String interpolation:**
   - `"${Num.to_str(birds)}"` → needs `Num.to_str` (builtin)
   - Produces: `"-3"`

5. **Function call:**
   - `Stdout.line!("There are -3 birds.")`
   - Look up `line!` in platform
   - Type: `Str => Result {} [StdoutErr IOErr]`
   - Argument: `"There are -3 birds."` (Str) ✓
   - Execute: Call Host.stdout_line! (Rust native code)
   - Output: `"There are -3 birds."` → stdout

---

## Benefits of This Approach

✅ **Zero-Copy:**
- Download once, keep in memory
- All strings interned (&'static str)
- Parse once, share via Rc<>

✅ **Efficient:**
- 14MB in memory is acceptable
- Fast lookups via HashMap
- No re-parsing

✅ **Scalable:**
- Works with any platform size
- Easy to add multiple platforms
- Lazy loading per module (v2.0)

✅ **Safe:**
- All references are &'static str
- No dangling pointers
- Rc handles lifetime management

---

## Comparison: Before/After

**Before (v1.0, without platforms):**
```roc
birds = -3
result = "There are ${I64.to_str(birds)} birds."
result
```
✓ Works, but no I/O

**After (v1.0 + Phase 16):**
```roc
app [main!] { pf: platform "https://...basic-cli...tar.br" }
import pf.Stdout

birds = -3

main! = |_args|
  Stdout.line!("There are ${Num.to_str(birds)} birds.")
```
✓ Full hello_world with I/O (minus effects in v2.0)

---

## Testing Checklist

- [ ] Platform downloads from URL
- [ ] Archive decompresses correctly
- [ ] Tar extracts modules
- [ ] .roc files parse correctly
- [ ] Module imports resolve
- [ ] Function calls find correct platform functions
- [ ] Type checking works
- [ ] Stdout.line! executes (prints to stdout)
- [ ] Caching works (no re-download)
- [ ] Multiple modules work together
