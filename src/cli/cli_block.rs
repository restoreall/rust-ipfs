use crate::{cli::handler, Block};
use cid::Cid;
use futures::executor;
use multihash::Sha2_256;
use std::convert::TryFrom;
use libp2p_rs::xcli::*;

pub(crate) fn cli_block_commands<'a>() -> Command<'a> {
    let get_block_cmd = Command::new_with_alias("get", "g")
        .about("Get a raw IPFS block")
        .usage("get <cid>")
        .action(cli_get_block);
    let put_block_cmd = Command::new_with_alias("put", "p")
        .about("Store input as an IPFS block")
        .usage("put <string>")
        .action(cli_put_block);
    let rm_block_cmd = Command::new("rm")
        .about("Remove IPFS block(s)")
        .usage("rm <cid>")
        .action(cli_remove_block);

    Command::new_with_alias("block", "b")
        .about("Interact with raw blocks in the datastore")
        .usage("ipfs block")
        .subcommand(get_block_cmd)
        .subcommand(put_block_cmd)
        .subcommand(rm_block_cmd)
}

fn cli_get_block(app: &App, args: &[&str]) -> XcliResult {
    if args.is_empty() {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let cid = Cid::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    executor::block_on(async {
        let r = ipfs.get_block(&cid).await;
        match r {
            Ok(data) => {
                println!("{} {:?}", data.cid(), data.data());
            }
            Err(e) => {
                println!("{:?}", e);
            }
        }
    });

    Ok(CmdExeCode::Ok)
}

fn cli_put_block(app: &App, args: &[&str]) -> XcliResult {
    if args.is_empty() {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let block = args[0].as_bytes();
    let cid = Cid::new_v1(cid::Codec::Raw, Sha2_256::digest(block));
    let block = Block {
        cid,
        data: block.into(),
    };

    executor::block_on(async {
        let r = ipfs.put_block(block).await;
        match r {
            Ok(cid) => println!("{}", cid),
            Err(err) => println!("{}", err),
        }
    });

    Ok(CmdExeCode::Ok)
}

fn cli_remove_block(app: &App, args: &[&str]) -> XcliResult {
    if args.is_empty() {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let cid = Cid::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    executor::block_on(async {
        let r = ipfs.remove_block(cid).await;
        if let Ok(cid) = r {
            println!("{} is removed", cid);
        }
    });

    Ok(CmdExeCode::Ok)
}
