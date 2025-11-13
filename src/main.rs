use clap::Parser;
use color_eyre::Result;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(name = "lit")]
#[command(about = "A literate programming tool", long_about = None)]
struct Args {
    /// Directory to process
    #[arg(value_name = "DIRECTORY")]
    directory: PathBuf,
}

/// Represents a code block with a tangle path
#[derive(Debug, Clone)]
struct TangleBlock {
    /// The file path where this code should be written
    path: PathBuf,
    /// The code content
    content: String,
}

/// Parse a markdown file and extract all tangle code blocks
fn parse_markdown_file(path: &PathBuf) -> Result<Vec<TangleBlock>> {
    let content = fs::read_to_string(path)?;
    Ok(parse_tangle_blocks(&content))
}

/// Parse markdown content and extract code blocks with tangle:// paths
fn parse_tangle_blocks(markdown: &str) -> Vec<TangleBlock> {
    let mut blocks = Vec::new();
    let mut in_code_block = false;
    let mut current_path: Option<PathBuf> = None;
    let mut current_content = String::new();

    for line in markdown.lines() {
        if line.starts_with("```") {
            if in_code_block {
                // End of code block
                if let Some(path) = current_path.take() {
                    blocks.push(TangleBlock {
                        path,
                        content: current_content.clone(),
                    });
                }
                current_content.clear();
                in_code_block = false;
            } else {
                // Start of code block
                let lang = line.trim_start_matches('`').trim();
                if let Some(path_str) = lang.strip_prefix("tangle://") {
                    current_path = Some(PathBuf::from(path_str));
                    in_code_block = true;
                }
            }
        } else if in_code_block {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    blocks
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    println!("Processing markdown files in {}:\n", args.directory.display());

    for entry in WalkDir::new(&args.directory)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "md" {
                    let path = entry.path().to_path_buf();
                    println!("File: {}", path.display());

                    match parse_markdown_file(&path) {
                        Ok(blocks) => {
                            if blocks.is_empty() {
                                println!("  No tangle blocks found");
                            } else {
                                println!("  Found {} tangle block(s):", blocks.len());
                                for block in blocks {
                                    println!("    → {}", block.path.display());
                                    println!("      {} lines", block.content.lines().count());
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("  Error parsing file: {}", e);
                        }
                    }
                    println!();
                }
            }
        }
    }

    Ok(())
}
