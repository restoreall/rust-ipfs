use ipfs::{Ipfs, IpfsOptions, TestTypes, UninitializedIpfs};


fn main() {
    futures::executor::block_on(entry());
}

async fn entry() {
    tracing_subscriber::fmt()
        //.with_max_level(tracing::Level::DEBUG)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Initialize the repo and start a daemon
    let mut opts = IpfsOptions::inmemory_with_generated_keys();

    opts.listening_addrs.push("/ip4/0.0.0.0/tcp/8084".parse().unwrap());
    //opts.bootstrap.push(("QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ".parse().unwrap(), "/ip4/104.131.131.82/tcp/4001".parse().unwrap()));
    opts.bootstrap.push(("12D3KooWDsgzyxLH2fTrFeqxJrFxBz4uMEvw4gbR6yeAChSW6ELe".parse().unwrap(), "/ip4/127.0.0.1/tcp/4001".parse().unwrap()));

    let ipfs: Ipfs<TestTypes> = UninitializedIpfs::new(opts).start().await.unwrap();

    let addrs = ipfs.identity().await.unwrap().1;
    println!("I am listening on {:?}", addrs);
    //
    // let mut cids = vec![];
    // for i in 0..250 {
    //
    //     let mut block = vec![];
    //     block.resize(1000000, i);
    //     let cid = Cid::new_v1(cid::Codec::Raw, Sha2_256::digest(block.as_ref()));
    //     let block = Block {
    //         cid: cid.clone(),
    //         data: block.into(),
    //     };
    //     ipfs.put_block_now(block).await.unwrap();
    //
    //     println!("cid {}", cid);
    //     cids.push(cid.clone());
    // }
    //
    // for cid in cids {
    //     let r = ipfs.get_block_now(&cid).await;
    //     println!("get cid {} {}", cid, r.unwrap_or_default().is_some());
    // }


    ipfs.run_cli().await;
}
