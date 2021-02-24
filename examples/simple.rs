use ipfs::{Ipfs, IpfsOptions, TestTypes, UninitializedIpfs};

#[tokio::main]
async fn main() {
    // tracing_subscriber::fmt()
    //     .with_max_level(tracing::Level::DEBUG)
    //     .init();
    env_logger::init();

    // Initialize the repo and start a daemon
    let mut opts = IpfsOptions::inmemory_with_generated_keys();

    opts.listening_addrs.push("/ip4/0.0.0.0/tcp/8084".parse().unwrap());
    //opts.bootstrap.push(("/ip4/104.131.131.82/tcp/4001".parse().unwrap(), "QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ".parse().unwrap()));
    opts.bootstrap.push(("/ip4/127.0.0.1/tcp/4001".parse().unwrap(), "12D3KooWDsgzyxLH2fTrFeqxJrFxBz4uMEvw4gbR6yeAChSW6ELe".parse().unwrap()));

    let ipfs: Ipfs<TestTypes> = UninitializedIpfs::new(opts).start().await.unwrap();

    let addrs = ipfs.identity().await.unwrap().1;
    println!("I am listening on {:?}", addrs);

    ipfs.run_cli().await;
}
