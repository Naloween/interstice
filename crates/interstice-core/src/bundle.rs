//! Bundles: a whole node — modules and optionally their data — in one file
//! (DESIGN §9.1). This module holds the format and the file-level helpers;
//! installing goes through [`crate::Node::start_bundle`].

use crate::error::IntersticeError;
use interstice_abi::ModuleSchema;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"ITBUNDLE";
const FORMAT_VERSION: u32 = 1;
/// Trailer of a program: `[bundle bytes][len: u64 LE][EXE_MAGIC]`.
const EXE_MAGIC: &[u8; 8] = b"ITBEXE01";
const TRAILER_LEN: u64 = 16;
/// The install record in a node directory.
pub const RECORD_FILE: &str = "bundle.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundleManifest {
    /// The node's name in the registry.
    pub name: String,
    pub version: String,
    pub description: String,
    pub port: Option<u32>,
    pub abi_version: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleFile {
    /// Path relative to the module directory, `/`-separated.
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleModule {
    /// Known for every bundle written to a file; `None` only for in-memory
    /// bundles (examples), where the runtime learns it at load.
    pub name: Option<String>,
    pub wasm: Vec<u8>,
    /// Seed data: the module directory's files, `module.wasm` excluded.
    pub data: Vec<BundleFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub manifest: BundleManifest,
    pub modules: Vec<BundleModule>,
}

/// What an install leaves in the node directory (`bundle.toml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallRecord {
    pub name: String,
    pub version: String,
    /// Names of the modules this bundle installed.
    pub modules: Vec<String>,
}

fn err(text: impl Into<String>) -> IntersticeError {
    IntersticeError::Internal(text.into())
}

impl Bundle {
    pub fn encode(&self) -> Result<Vec<u8>, IntersticeError> {
        for module in &self.modules {
            if module.name.is_none() {
                return Err(err("every module of a bundle file needs a name"));
            }
        }
        let payload = interstice_abi::encode(self).map_err(|e| err(format!("bundle encode: {e}")))?;
        let compressed =
            zstd::encode_all(&payload[..], 9).map_err(|e| err(format!("bundle compress: {e}")))?;
        let mut out = Vec::with_capacity(compressed.len() + 12);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&compressed);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, IntersticeError> {
        if bytes.len() < 12 || &bytes[..8] != MAGIC {
            return Err(err("not an Interstice bundle"));
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if version != FORMAT_VERSION {
            return Err(err(format!(
                "bundle format {version} is not supported (this interstice reads {FORMAT_VERSION})"
            )));
        }
        let payload =
            zstd::decode_all(&bytes[12..]).map_err(|e| err(format!("bundle decompress: {e}")))?;
        interstice_abi::decode(&payload).map_err(|e| err(format!("bundle decode: {e}")))
    }

    pub fn read(path: &Path) -> Result<Self, IntersticeError> {
        // A program is accepted too: its bundle is in the trailer.
        if let Some(bundle) = read_from_exe(path)? {
            return Ok(bundle);
        }
        let bytes = std::fs::read(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
        Self::decode(&bytes)
    }
}

/// The bundle appended to the program at `path`, if there is one.
pub fn read_from_exe(path: &Path) -> Result<Option<Bundle>, IntersticeError> {
    let Some((start, len)) = exe_trailer(path)? else { return Ok(None) };
    let mut file = std::fs::File::open(path).map_err(|e| err(e.to_string()))?;
    file.seek(SeekFrom::Start(start)).map_err(|e| err(e.to_string()))?;
    let mut bytes = vec![0u8; len as usize];
    file.read_exact(&mut bytes).map_err(|e| err(e.to_string()))?;
    Bundle::decode(&bytes).map(Some)
}

/// `(offset, length)` of the bundle in a program, from its trailer.
fn exe_trailer(path: &Path) -> Result<Option<(u64, u64)>, IntersticeError> {
    let Ok(mut file) = std::fs::File::open(path) else { return Ok(None) };
    let size = file.metadata().map_err(|e| err(e.to_string()))?.len();
    if size < TRAILER_LEN {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(size - TRAILER_LEN)).map_err(|e| err(e.to_string()))?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    file.read_exact(&mut trailer).map_err(|e| err(e.to_string()))?;
    if &trailer[8..] != EXE_MAGIC {
        return Ok(None);
    }
    let len = u64::from_le_bytes(trailer[..8].try_into().unwrap());
    if len > size - TRAILER_LEN {
        return Err(err("corrupt program trailer"));
    }
    Ok(Some((size - TRAILER_LEN - len, len)))
}

/// Write a program: the binary at `exe` (without any bundle it already
/// carries), then `bundle`, then the trailer.
pub fn write_exe(exe: &Path, bundle: &Bundle, out: &Path) -> Result<(), IntersticeError> {
    let mut binary = std::fs::read(exe).map_err(|e| err(format!("{}: {e}", exe.display())))?;
    if let Some((start, _)) = exe_trailer(exe)? {
        binary.truncate(start as usize);
    }
    let bytes = bundle.encode()?;
    binary.extend_from_slice(&bytes);
    binary.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    binary.extend_from_slice(EXE_MAGIC);
    std::fs::write(out, &binary).map_err(|e| err(format!("{}: {e}", out.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Entries of a module directory that are never seed data.
const SKIP: &[&str] = &["module.wasm", ".migrate"];

/// The loaded modules of the node directory `node_dir`, with their data
/// when `with_data`. The node must be stopped.
pub fn modules_from_node(node_dir: &Path, with_data: bool) -> Result<Vec<BundleModule>, IntersticeError> {
    let modules_dir = node_dir.join("modules");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&modules_dir)
        .map_err(|e| err(format!("{}: {e}", modules_dir.display())))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && p.join("module.wasm").exists())
        .collect();
    entries.sort();
    let mut modules = Vec::new();
    for dir in entries {
        if !crate::runtime::module::module_should_load(&dir) {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let wasm = std::fs::read(dir.join("module.wasm")).map_err(|e| err(e.to_string()))?;
        let mut data = Vec::new();
        if with_data {
            collect_files(&dir, &dir, &mut data)?;
        }
        modules.push(BundleModule { name: Some(name), wasm, data });
    }
    Ok(modules)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<BundleFile>) -> Result<(), IntersticeError> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| err(e.to_string()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries {
        let rel = path.strip_prefix(root).unwrap();
        if dir == root && SKIP.iter().any(|s| rel == Path::new(s)) {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            out.push(BundleFile { path: rel, bytes: std::fs::read(&path).map_err(|e| err(e.to_string()))? });
        }
    }
    Ok(())
}

/// Write seed `files` into `module_dir`. Paths are checked to stay inside.
pub(crate) fn write_seed(module_dir: &Path, files: &[BundleFile]) -> Result<(), IntersticeError> {
    for file in files {
        let rel = Path::new(&file.path);
        if rel.is_absolute()
            || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(err(format!("bundle data path '{}' leaves the module directory", file.path)));
        }
        let path = module_dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| err(e.to_string()))?;
        }
        std::fs::write(&path, &file.bytes).map_err(|e| err(format!("{}: {e}", path.display())))?;
    }
    Ok(())
}

pub fn read_record(node_dir: &Path) -> Option<InstallRecord> {
    let text = std::fs::read_to_string(node_dir.join(RECORD_FILE)).ok()?;
    toml::from_str(&text).ok()
}

pub(crate) fn write_record(node_dir: &Path, record: &InstallRecord) -> Result<(), IntersticeError> {
    let text = toml::to_string_pretty(record).map_err(|e| err(e.to_string()))?;
    std::fs::write(node_dir.join(RECORD_FILE), text).map_err(|e| err(e.to_string()))
}

/// A module's schema without a node: every import is stubbed as a trap,
/// since `interstice_describe` calls no host function.
pub fn describe_wasm(wasm: &[u8]) -> Result<ModuleSchema, IntersticeError> {
    use wasmtime::{Config, Engine, Linker, Module, OptLevel, Store};
    let mut config = Config::new();
    config.cranelift_opt_level(OptLevel::None);
    let engine = Engine::new(&config).map_err(|e| err(e.to_string()))?;
    let module = Module::new(&engine, wasm).map_err(|e| err(format!("invalid wasm module: {e}")))?;
    let mut linker: Linker<()> = Linker::new(&engine);
    linker
        .define_unknown_imports_as_traps(&module)
        .map_err(|e| err(e.to_string()))?;
    let mut store = Store::new(&engine, ());
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| err(format!("wasm instantiation failed: {e}")))?;
    let describe = instance
        .get_typed_func::<(), i64>(&mut store, "interstice_describe")
        .map_err(|_| IntersticeError::MissingExport("interstice_describe"))?;
    let packed = describe.call(&mut store, ()).map_err(|e| IntersticeError::WasmTrap(e.to_string()))?;
    let (ptr, len) = ((packed >> 32) as u32 as usize, (packed & 0xffff_ffff) as u32 as usize);
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or(IntersticeError::MissingExport("memory"))?;
    let mut bytes = vec![0u8; len];
    memory.read(&store, ptr, &mut bytes).map_err(|_| IntersticeError::MemoryRead)?;
    interstice_abi::decode(&bytes).map_err(|e| err(format!("schema decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Bundle {
        Bundle {
            manifest: BundleManifest { name: "game".into(), version: "1".into(), abi_version: 13, ..Default::default() },
            modules: vec![BundleModule {
                name: Some("m".into()),
                wasm: vec![0, 97, 115, 109],
                data: vec![BundleFile { path: "stateful/t/1.row".into(), bytes: vec![1, 2, 3] }],
            }],
        }
    }

    #[test]
    fn bundle_round_trip_and_program_trailer() {
        let dir = std::env::temp_dir().join(format!("itb-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bundle = sample();
        let decoded = Bundle::decode(&bundle.encode().unwrap()).unwrap();
        assert_eq!(decoded.manifest.name, "game");
        assert_eq!(decoded.modules[0].data[0].bytes, vec![1, 2, 3]);

        // A program: binary + bundle + trailer; re-wrapping replaces the bundle.
        let exe = dir.join("bin");
        std::fs::write(&exe, b"ELF-binary").unwrap();
        assert!(read_from_exe(&exe).unwrap().is_none());
        let prog = dir.join("prog");
        write_exe(&exe, &bundle, &prog).unwrap();
        assert_eq!(read_from_exe(&prog).unwrap().unwrap().manifest.name, "game");
        let mut other = sample();
        other.manifest.name = "other".into();
        let prog2 = dir.join("prog2");
        write_exe(&prog, &other, &prog2).unwrap();
        let bytes = std::fs::read(&prog2).unwrap();
        assert!(bytes.starts_with(b"ELF-binary") && !bytes[10..].starts_with(b"ELF"));
        assert_eq!(Bundle::read(&prog2).unwrap().manifest.name, "other");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn seed_paths_stay_inside() {
        let dir = std::env::temp_dir().join(format!("itb-seed-{}", uuid::Uuid::new_v4()));
        let bad = [BundleFile { path: "../escape".into(), bytes: vec![] }];
        assert!(write_seed(&dir, &bad).is_err());
        write_seed(&dir, &sample().modules[0].data).unwrap();
        assert!(dir.join("stateful/t/1.row").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
