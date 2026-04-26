use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use wasmtime_wasi::WasiCtx;
use wasmtime::component::ResourceTable;

pub struct HostModuleLoader {
    pub search_paths: Arc<RwLock<Vec<PathBuf>>>,
}

pub struct HostState {
    pub ctx: WasiCtx,
    pub table: ResourceTable,
    pub loader: HostModuleLoader,
}
