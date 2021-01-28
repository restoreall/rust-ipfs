use ipfs::{Ipfs, IpfsOptions, IpfsPath, TestTypes, UninitializedIpfs};
use tokio::task;
use xcli::App;
use ipfs::cli::ipfs_cli_commands;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Initialize the repo and start a daemon
    let opts = IpfsOptions::inmemory_with_generated_keys();
    let (ipfs, fut): (Ipfs<TestTypes>, _) = UninitializedIpfs::new(opts).start().await.unwrap();
    task::spawn(fut);

    let mut app = App::new("xCLI");
    app.add_subcommand_with_userdata(ipfs_cli_commands(), Box::new(ipfs));

    app.run();
}