//! A headless node running the `bench` module, driven in process.

use interstice_core::{IntersticeError, Node, NodeHandle, interstice_abi::IntersticeValue};
use std::path::PathBuf;

/// The bench module's name.
pub const MODULE: &str = "bench";

/// Where the bench module's wasm is: `INTERSTICE_BENCH_WASM`, or the
/// workspace's release build (`scripts/build_modules.sh` builds it).
pub fn wasm_path() -> PathBuf {
    if let Ok(path) = std::env::var("INTERSTICE_BENCH_WASM") {
        return path.into();
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-unknown-unknown/release/bench.wasm")
}

pub fn wasm() -> Vec<u8> {
    let path = wasm_path();
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read the bench module at {} ({e}); build it with \
             `cargo build -p bench --target wasm32-unknown-unknown --release`",
            path.display()
        )
    })
}

/// A node in a temporary directory, with its own tokio runtime.
pub struct BenchNode {
    pub handle: NodeHandle,
    pub rt: tokio::runtime::Runtime,
    _dir: tempfile::TempDir,
}

impl BenchNode {
    pub fn start() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let wasm = wasm();
        let handle = rt.block_on(async {
            let node = Node::new(dir.path(), 0, "127.0.0.1:0".into()).expect("node");
            node.start_headless(&[&wasm]).await.expect("start node")
        });
        Self { handle, rt, _dir: dir }
    }

    /// Call a reducer of the bench module and wait for its commit.
    pub fn call(&self, reducer: &str, args: Vec<IntersticeValue>) -> Result<(), IntersticeError> {
        self.rt
            .block_on(self.handle.call_reducer(MODULE, reducer, IntersticeValue::Vec(args)))
    }

    pub fn query(&self, query: &str, args: Vec<IntersticeValue>) -> Result<IntersticeValue, IntersticeError> {
        self.rt
            .block_on(self.handle.call_query(MODULE, query, IntersticeValue::Vec(args)))
    }
}
