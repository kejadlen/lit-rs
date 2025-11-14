use clap::Parser;
use color_eyre::Result;
use lit::Lit;
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "lit")]
#[command(about = "A literate programming tool", long_about = None)]
struct Args {
    /// Input directory to process
    #[arg(value_name = "INPUT")]
    directory: PathBuf,

    /// Output directory for tangled files (defaults to INPUT/out)
    #[arg(value_name = "OUTPUT")]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let output = args.output.unwrap_or_else(|| args.directory.join("out"));

    let input_display = args.directory.display();
    let output_display = output.display();
    info!("Reading markdown files from: {input_display}");
    info!("Writing tangled files to: {output_display}");

    let lit = Lit::new(args.directory, output);
    lit.tangle()?;

    info!("Tangling complete!");

    Ok(())
}
