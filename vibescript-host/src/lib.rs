use wasmtime::{Engine, component::Linker};

wasmtime::component::bindgen!({
    world: "vibes-provider",
    path: "wit",
    exports: {
        "xipkit:vibes/engine-world": async,
    },
});

pub struct VibesHost {
    engine: Engine,
    linker: Linker<HostState>,
}

const VIBES_CORE_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vibescript_core.wasm"));

impl VibesHost {
    pub fn new() -> anyhow::Result<VibesHost> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);

        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;

        Ok(Self { engine, linker })
    }

    pub async fn execute(&self, source: &str) -> anyhow::Result<String> {
        let wasi = wasmtime_wasi::WasiCtxBuilder::new()
            .inherit_stdout()
            .inherit_stderr()
            .build();

        let mut store = wasmtime::Store::new(
            &self.engine,
            HostState {
                ctx: wasi,
                table: wasmtime::component::ResourceTable::new(),
            },
        );

        let component = wasmtime::component::Component::from_binary(&self.engine, VIBES_CORE_WASM)?;

        let instance =
            VibesProvider::instantiate_async(&mut store, &component, &self.linker).await?;

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
            .map_err(|e| anyhow::anyhow!("Compilation failed: {}", e))?;

        // Execute script
        let result = engine_world
            .script()
            .call_call(&mut store, script, "", &[], &[])
            .await?
            .map_err(|e| anyhow::anyhow!("Execution failed: {}", e))?;

        Ok(format!("{:?}", result))
    }
}

pub struct HostState {
    pub ctx: wasmtime_wasi::WasiCtx,
    pub table: wasmtime_wasi::ResourceTable,
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
        let result = host.execute(source).await?;
        assert!(
            result.contains("I(3)"),
            "Result should contain I(3), but got: {}",
            result
        );
        Ok(())
    }
}
