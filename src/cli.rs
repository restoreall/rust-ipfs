use cid::Cid;
use crate::{Ipfs, TestTypes, Block};
use futures::executor;
use xcli::*;
use std::convert::TryFrom;
use multihash::Sha2_256;


const IPFS: &str = "ipfs";

pub fn ipfs_cli_commands<'a>() -> Command<'a> {
    let get_block_cmd = Command::new("get")
        .about("get block")
        .usage("get <cid>")
        .action(cli_get_block);
    let put_block_cmd = Command::new("put")
        .about("put block")
        .usage("put <string>")
        .action(cli_put_block);

    Command::new_with_alias(IPFS, "i")
        .about("IPFS")
        .usage("ipfs")
        .subcommand(get_block_cmd)
        .subcommand(put_block_cmd)
}

fn handler(app: &App) -> Ipfs<TestTypes>
{
    let value_any = app.get_handler(IPFS).expect(IPFS);
    let ipfs = value_any.downcast_ref::<Ipfs<TestTypes>>().expect("ipfs").clone();
    ipfs
}

fn cli_get_block(app: &App, args: &[&str]) -> XcliResult {
    let ipfs = handler(app);

    let cid = if args.len() == 1 {
        Cid::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?
    } else {
        return Err(XcliError::MismatchArgument(1, args.len()));
    };

    executor::block_on(async {
        let r = ipfs.get_block(&cid).await;
        if let Ok(data) = r {
            println!("{} {:?}", data.cid(), data.data());
        }
    });

    Ok(CmdExeCode::Ok)
}

fn cli_put_block(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MissingArgument);
    }

    let ipfs = handler(app);
    let block = args[0].as_bytes();
    let cid = Cid::new_v0(Sha2_256::digest(block)).unwrap();
    let block = Block {
        cid: cid.clone(),
        data: block.into(),
    };

    executor::block_on(async {
        let data = ipfs.put_block(block).await;
        println!("{} {:?}", cid, data);
    });

    Ok(CmdExeCode::Ok)
}