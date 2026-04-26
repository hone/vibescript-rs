use std::sync::{Arc, RwLock};
use wasmtime::{Engine as WasmEngine, component::Linker, component::ResourceTable};
use wasmtime_wasi::WasiCtx;

wasmtime::component::bindgen!({
    world: "engine",
    path: "wit",
    exports: {
        "xipkit:vibes/engine-world": async,
    },
});

pub struct HostState {
    pub ctx: WasiCtx,
    pub table: ResourceTable,
    pub loader: HostModuleLoader,
}

pub struct HostModuleLoader {
    pub search_paths: Arc<RwLock<Vec<std::path::PathBuf>>>,
}

pub struct VibesHost {
    engine: WasmEngine,
    linker: Linker<HostState>,
    component: wasmtime::component::Component,
    pub search_paths: Arc<RwLock<Vec<std::path::PathBuf>>>,
}

const VIBES_CORE_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vibescript_core.wasm"));

impl VibesHost {
    pub fn new() -> anyhow::Result<VibesHost> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);

        let engine = WasmEngine::new(&config)?;
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;

        Engine::add_to_linker::<HostState, HostState>(&mut linker, |s| s)?;

        let component = wasmtime::component::Component::from_binary(&engine, VIBES_CORE_WASM)?;

        Ok(Self {
            engine,
            linker,
            component,
            search_paths: Arc::new(RwLock::new(vec![std::env::current_dir()?])),
        })
    }

    async fn setup_instance(&self) -> anyhow::Result<(wasmtime::Store<HostState>, Engine)> {
        let wasi = wasmtime_wasi::WasiCtxBuilder::new()
            .inherit_stdout()
            .inherit_stderr()
            .build();

        let mut store = wasmtime::Store::new(
            &self.engine,
            HostState {
                ctx: wasi,
                table: ResourceTable::new(),
                loader: HostModuleLoader {
                    search_paths: self.search_paths.clone(),
                },
            },
        );

        let instance = Engine::instantiate_async(&mut store, &self.component, &self.linker).await?;

        Ok((store, instance))
    }

    pub async fn check(&self, source: &str) -> anyhow::Result<()> {
        let (mut store, instance) = self.setup_instance().await?;
        let engine_world = instance.xipkit_vibes_engine_world();

        let cfg = exports::xipkit::vibes::engine_world::EngineConfig {
            max_steps: 1000000,
            max_memory_bytes: 10 * 1024 * 1024,
        };

        let engine_res = engine_world
            .engine()
            .call_constructor(&mut store, cfg)
            .await?;

        // Compile source
        let _script = engine_world
            .engine()
            .call_compile(&mut store, engine_res, source)
            .await?
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(())
    }

    pub async fn execute(
        &self,
        source: &str,
        function_name: &str,
        args: &[String],
    ) -> anyhow::Result<String> {
        let (mut store, instance) = self.setup_instance().await?;
        let engine_world = instance.xipkit_vibes_engine_world();

        let cfg = exports::xipkit::vibes::engine_world::EngineConfig {
            max_steps: 1000000,
            max_memory_bytes: 10 * 1024 * 1024,
        };

        // Create engine resource
        let engine_res = engine_world
            .engine()
            .call_constructor(&mut store, cfg)
            .await?;

        // Compile source
        let script = engine_world
            .engine()
            .call_compile(&mut store, engine_res, source)
            .await?
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Convert string args to WitValue, trying to parse as numbers or JSON first
        let wit_args: Vec<exports::xipkit::vibes::engine_world::Value> = args
            .iter()
            .map(|s| {
                if let Ok(i) = s.parse::<i64>() {
                    exports::xipkit::vibes::engine_world::Value::I(i)
                } else if let Ok(f) = s.parse::<f64>() {
                    exports::xipkit::vibes::engine_world::Value::F(f)
                } else if (s.starts_with('[') && s.ends_with(']'))
                    || (s.starts_with('{') && s.ends_with('}'))
                {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                        exports::xipkit::vibes::engine_world::Value::Json(v.to_string())
                    } else {
                        exports::xipkit::vibes::engine_world::Value::S(s.clone())
                    }
                } else {
                    exports::xipkit::vibes::engine_world::Value::S(s.clone())
                }
            })
            .collect();

        // Execute script using the specified function name
        let result = engine_world
            .script()
            .call_call(&mut store, script, function_name, &wit_args, &[])
            .await?
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(format!("{:?}", result))
    }
}

impl HostModuleLoader {
    fn load_module_impl(
        &mut self,
        name: String,
        caller_path: Option<String>,
    ) -> Result<(String, String), String> {
        let mut name_with_ext = name.clone();
        if !name_with_ext.ends_with(".vibe") {
            name_with_ext.push_str(".vibe");
        }

        let mut resolved_path = None;

        // 1. Check relative path if it's an explicit relative request
        if name.starts_with("./") || name.starts_with("../") {
            if let Some(caller) = caller_path {
                if let Some(caller_dir) = std::path::Path::new(&caller).parent() {
                    let candidate = caller_dir.join(&name_with_ext);
                    if candidate.exists() {
                        resolved_path = Some(candidate);
                    }
                }
            }
        }

        // 2. Check search paths
        if resolved_path.is_none() {
            let paths = self.search_paths.read().unwrap();
            for path in paths.iter() {
                let candidate = path.join(&name_with_ext);
                if candidate.exists() {
                    resolved_path = Some(candidate);
                    break;
                }
            }
        }

        match resolved_path {
            Some(path) => {
                let abs_path = match std::fs::canonicalize(&path) {
                    Ok(p) => p,
                    Err(e) => return Err(format!("Failed to resolve path: {}", e)),
                };

                // Security Sandbox: Ensure the canonical path starts with one of the search paths
                let paths = self.search_paths.read().unwrap();
                let mut allowed = false;
                for search_path in paths.iter() {
                    let canonical_search = match std::fs::canonicalize(search_path) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    if abs_path.starts_with(&canonical_search) {
                        allowed = true;
                        break;
                    }
                }

                if !allowed {
                    return Err(format!(
                        "Security error: Module path '{}' is outside of allowed search paths",
                        abs_path.display()
                    ));
                }

                let source = match std::fs::read_to_string(&abs_path) {
                    Ok(s) => s,
                    Err(e) => return Err(format!("Failed to read module: {}", e)),
                };
                Ok((source, abs_path.to_string_lossy().to_string()))
            }
            None => Err(format!("Module '{}' not found", name)),
        }
    }
}

impl xipkit::vibes::loader::Host for HostState {
    fn load_module(
        &mut self,
        name: String,
        caller_path: Option<String>,
    ) -> Result<(String, String), String> {
        self.loader.load_module_impl(name, caller_path)
    }
}

// Map HostState to itself for HasData
impl wasmtime::component::HasData for HostState {
    type Data<'a> = &'a mut HostState;
}

impl wasmtime_wasi::WasiView for HostState {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_execution() -> anyhow::Result<()> {
        let host = VibesHost::new()?;
        // A simple vibescript expression
        let source = "1 + 2";
        // Passing empty function name to trigger fallback to last value
        let result = host.execute(source, "", &[]).await?;
        assert!(
            result.contains("I(3)"),
            "Result should contain I(3), but got: {}",
            result
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_function_call() -> anyhow::Result<()> {
        let host = VibesHost::new()?;
        let source = "def greet(name)\n  \"hello \" + name\nend";
        let result = host.execute(source, "greet", &["Gwen".to_string()]).await?;
        assert!(
            result.contains("S(\"hello Gwen\")"),
            "Result should contain hello Gwen, but got: {}",
            result
        );
        Ok(())
    }
}
