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
    component: wasmtime::component::Component,
}

const VIBES_CORE_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vibescript_core.wasm"));

impl VibesHost {
    pub fn new() -> anyhow::Result<VibesHost> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);

        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;

        let component = wasmtime::component::Component::from_binary(&engine, VIBES_CORE_WASM)?;

        Ok(Self {
            engine,
            linker,
            component,
        })
    }

    async fn setup_instance(&self) -> anyhow::Result<(wasmtime::Store<HostState>, VibesProvider)> {
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

        let instance =
            VibesProvider::instantiate_async(&mut store, &self.component, &self.linker).await?;

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

        // Convert string args to WitValue::S
        let wit_args: Vec<exports::xipkit::vibes::engine_world::Value> = args
            .iter()
            .map(|s| exports::xipkit::vibes::engine_world::Value::S(s.clone()))
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
