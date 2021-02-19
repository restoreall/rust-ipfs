use ipfs::{Ipfs, IpfsOptions, TestTypes, UninitializedIpfs};
use tokio::task;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    // Initialize the repo and start a daemon
    let mut opts = IpfsOptions::inmemory_with_generated_keys();

    opts.listening_addrs.push("/ip4/0.0.0.0/tcp/8086".parse().unwrap());
    opts.bootstrap = vec![("/ip4/104.131.131.82/tcp/4001".parse().unwrap(), "QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ".parse().unwrap())];

    let (mut ipfs, fut): (Ipfs<TestTypes>, _) = UninitializedIpfs::new(opts).start().await.unwrap();
    task::spawn(fut);

    let addrs = ipfs.identity().await.unwrap().1;
    println!("I am listening on {:?}", addrs);

    ipfs.run_cli().await;
}
