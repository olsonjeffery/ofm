pub mod agent;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "omprint")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
}

#[derive(Subcommand)]
pub enum AgentAction {
    CompletePlan { task_id: String },
    CompleteWorkflow { task_id: String },
    BlockWorkflow { task_id: String },
    CompletePr { task_id: String },
}
