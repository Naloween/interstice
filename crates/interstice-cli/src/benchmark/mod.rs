mod command;
mod network;
mod report;
mod runner;
mod template;
mod types;
mod util;

use interstice_core::IntersticeError;

pub async fn handle_benchmark_command(args: &[String]) -> Result<(), IntersticeError> {
    command::handle_benchmark_command(args.get(2..).unwrap_or(&[]).to_vec()).await
}
