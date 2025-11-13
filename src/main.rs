use clap::Parser;
use color_eyre::Result;
use markdown::{to_mdast, ParseOptions};
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
fn parse_tangle_blocks(markdown_text: &str) -> Vec<TangleBlock> {
    let mut blocks = Vec::new();

    // Parse markdown to AST
    let ast = match to_mdast(markdown_text, &ParseOptions::default()) {
        Ok(ast) => ast,
        Err(_) => return blocks,
    };

    // Walk the AST to find code blocks
    extract_tangle_blocks_from_node(&ast, &mut blocks);

    blocks
}

/// Recursively extract tangle blocks from AST nodes
fn extract_tangle_blocks_from_node(node: &markdown::mdast::Node, blocks: &mut Vec<TangleBlock>) {
    use markdown::mdast::Node;

    match node {
        Node::Code(code) => {
            // Check if this is a tangle code block
            if let Some(lang) = &code.lang {
                if let Some(path_str) = lang.strip_prefix("tangle://") {
                    blocks.push(TangleBlock {
                        path: PathBuf::from(path_str),
                        content: code.value.clone(),
                    });
                }
            }
        }
        // Recursively process nodes that can contain children
        Node::Root(root) => {
            for child in &root.children {
                extract_tangle_blocks_from_node(child, blocks);
            }
        }
        Node::Blockquote(bq) => {
            for child in &bq.children {
                extract_tangle_blocks_from_node(child, blocks);
            }
        }
        Node::List(list) => {
            for child in &list.children {
                extract_tangle_blocks_from_node(child, blocks);
            }
        }
        Node::ListItem(item) => {
            for child in &item.children {
                extract_tangle_blocks_from_node(child, blocks);
            }
        }
        _ => {
            // Other node types either don't have children or don't need processing
        }
    }
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
