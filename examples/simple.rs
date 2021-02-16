use ipfs::cli::ipfs_cli_commands;
use ipfs::{Ipfs, IpfsOptions, TestTypes, UninitializedIpfs};
use tokio::task;
use xcli::App;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Initialize the repo and start a daemon
    let opts = IpfsOptions::inmemory_with_generated_keys();
    let (mut ipfs, fut): (Ipfs<TestTypes>, _) = UninitializedIpfs::new(opts).start().await.unwrap();
    task::spawn(fut);

    let addrs = ipfs.identity().await.unwrap().1;
    println!("I am listening on {:?}", addrs);

    let mut app = App::new("xCLI");
    app.add_subcommand_with_userdata(ipfs_cli_commands(), Box::new(ipfs));

    app.run();
}
