use std::fs;

use clap::{Parser, Subcommand};
use ske::{RuleSelector, list_entries, create_rule, remove_rule, list_rules};

#[derive(Parser)]
#[command(name = "skeci")]
struct Cli {
    /// What to look for
    #[arg(long, default_value = "rust_binary")]
    lookfor: String,

    /// Attribute type
    #[arg(long, default_value = "srcs")]
    attr: String,

    /// Target a specific rule by name (required when multiple rules of the same type exist)
    #[arg(long)]
    name: Option<String>,

    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    Add {
        /// The input file
        input: String,
        /// One or more files to process
        #[arg(required = true, num_args = 1..)]
        files: Vec<String>,
    },
    Remove {
        /// The input file
        input: String,
        /// One or more files to process
        #[arg(required = true, num_args = 1..)]
        files: Vec<String>,
    },
    List {
        /// The input file
        input: String,
    },
    Create {
        /// The input file
        input: String,
        /// The name for the new rule
        #[arg(long)]
        name: String,
    },
    Delete {
        /// The input file
        input: String,
        /// The name of the rule to remove
        #[arg(long)]
        name: String,
    },
    Rules {
        /// The input file
        input: String,
        /// Output in buck2/bazel format (//package:name)
        #[arg(long)]
        buck: bool,
    },
}


fn main() {
    let cli = Cli::parse();

    let selector = RuleSelector {
        rule_name: cli.lookfor.clone(),
        attr: cli.attr.clone(),
        name: cli.name.clone(),
    };

    match cli.action {
        Action::Add { input: input_path, files } => {
            let input = fs::read_to_string(&input_path);
            if input.is_err() {
                println!("This has to be a valid file");
                return;
            }
            let mut input = input.unwrap();
            add_entries(&mut input, &selector, files);
            let _ = fs::write(&input_path, &input);
        }
        Action::Remove { input: input_path, files } => {
            let input = fs::read_to_string(&input_path);
            if input.is_err() {
                println!("This has to be a valid file");
                return;
            }
            let mut input = input.unwrap();
            remove_entries(&mut input, &selector, files);
            let _ = fs::write(&input_path, &input);
        }
        Action::List { input: input_path } => {
            let input = fs::read_to_string(&input_path);
            if input.is_err() {
                println!("Unable to read files from:{}", &input_path);
                return;
            }
            let input = input.unwrap();
            let files = list_entries(&input, &selector);
            if files.is_err() {
                println!("Unable to read files from:{}", &input_path);
                return;
            }
            for file in files.unwrap() {
                println!("{}", file);
            }
        }
        Action::Create { input: input_path, name } => {
            let input = fs::read_to_string(&input_path).unwrap_or_default();
            match create_rule(&input, &cli.lookfor, &name) {
                Ok(result) => {
                    let _ = fs::write(&input_path, &result);
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
            }
        }
        Action::Delete { input: input_path, name } => {
            let input = fs::read_to_string(&input_path);
            if input.is_err() {
                println!("This has to be a valid file");
                return;
            }
            let input = input.unwrap();
            match remove_rule(&input, &cli.lookfor, &name) {
                Ok(result) => {
                    let _ = fs::write(&input_path, &result);
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
            }
        }
        Action::Rules { input: input_path, buck } => {
            let input = fs::read_to_string(&input_path);
            if input.is_err() {
                println!("This has to be a valid file");
                return;
            }
            let input = input.unwrap();
            match list_rules(&input) {
                Ok(rules) => {
                    let package = if buck {
                        std::path::Path::new(&input_path)
                            .parent()
                            .and_then(|p| p.to_str())
                            .unwrap_or("")
                            .to_string()
                    } else {
                        String::new()
                    };
                    for rule in rules {
                        let name = rule.name.as_deref().unwrap_or("<unnamed>");
                        if buck {
                            println!("//{}:{}", package, name);
                        } else {
                            println!("{}", name);
                        }
                    }
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
            }
        }
    }
}


fn add_entries(input:&mut String, rule:&RuleSelector, files:Vec<String>) {
    for file in files  {
        let edit = ske::add_entry(&input, &rule, &file);
        if edit.is_err(){
            println!("Unable to adding to {}", file);
            return;
        }
        *input = edit.unwrap();
    }
}

fn remove_entries(input:&mut String, rule:&RuleSelector, files:Vec<String>) {
    for file in files  {
        let edit = ske::remove_entry(&input, &rule, &file);
        if edit.is_err(){
            println!("Unable to remove to {}", file);
            return;
        }
        *input = edit.unwrap();
    }
}