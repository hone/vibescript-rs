use anyhow::Context;
use clap::{Parser, Subcommand};
use vibescript_host::VibesHost;

#[derive(Parser)]
#[command(name = "vibes")]
#[command(about = "VibeScript CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute a script file
    Run {
        /// The script file to run
        script: String,

        /// Arguments to pass to the function
        args: Vec<String>,

        /// Function to invoke after compilation
        #[arg(short, long, default_value = "run")]
        function: String,

        /// Only compile the script without executing
        #[arg(short, long)]
        check: bool,
    },
    /// Start interactive REPL
    Repl,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            script,
            args,
            function,
            check,
        } => {
            let source = std::fs::read_to_string(&script)
                .with_context(|| format!("Failed to read script file: {}", script))?;

            let host = VibesHost::new().context("Failed to initialize VibeScript host")?;

            if check {
                host.check(&source).await.context("Check failed")?;
                println!("Check successful.");
            } else {
                let result = host
                    .execute(&source, &function, &args)
                    .await
                    .context("Execution failed")?;
                println!("{}", result);
            }
        }
        Commands::Repl => {
            anyhow::bail!("REPL is not implemented yet in the Rust version.");
        }
    }

    Ok(())
}
