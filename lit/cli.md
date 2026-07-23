# CLI Application

The command-line interface for the lit tool, implemented in `src/main.rs`.

## Main Entry Point

The CLI uses `clap` for argument parsing and provides a simple interface for tangling markdown files:

```tangle:///src/main.rs
use camino::Utf8PathBuf;
use clap::Parser;
use lit::Lit;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "lit")]
#[command(about = "A literate programming tool", long_about = None)]
struct Args {
    /// Input directory to process
    #[arg(value_name = "INPUT")]
    directory: Utf8PathBuf,

    /// Output directory for tangled files (defaults to INPUT/out)
    #[arg(value_name = "OUTPUT")]
    output: Option<Utf8PathBuf>,
}

fn main() -> miette::Result<()> {
    miette::set_panic_hook();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let output = args.output.unwrap_or_else(|| args.directory.join("out"));

    let input = &args.directory;
    info!("Reading markdown files from: {input}");
    info!("Writing tangled files to: {output}");

    let lit = Lit::new(args.directory, output);
    lit.tangle()?;

    info!("Tangling complete!");

    Ok(())
}
```
