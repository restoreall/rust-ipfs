use crate::cli::handler;
use futures::executor;
use libp2p::core::Multiaddr;
use std::str::FromStr;
use xcli::*;

pub(crate) fn cli_listen_commands<'a>() -> Command<'a> {
    let add_cmd = Command::new_with_alias("add", "a")
        .about("Add address to the listen address list.")
        .usage("ipfs listen add [<addr>]...")
        .action(cli_listen_add);
    let rm_cmd = Command::new_with_alias("rm", "r")
        .about("Remove address from the listen address list.")
        .usage("ipfs listen rm [<addr>]...")
        .action(cli_listen_rm);

    Command::new_with_alias("listen", "lis")
        .about("Add or remove listen address")
        .usage("ipfs listen")
        .subcommand(add_cmd)
        .subcommand(rm_cmd)
}

fn cli_listen_add(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let mut addrs = vec![];
    for arg in args {
        let addr =
            Multiaddr::from_str(arg.clone()).map_err(|e| XcliError::BadArgument(e.to_string()))?;
        addrs.push(addr);
    }

    executor::block_on(async {
        for addr in addrs {
            let r = ipfs.add_listening_address(addr).await;
            println!("added {:?}", r);
        }
    });

    Ok(CmdExeCode::Ok)
}

fn cli_listen_rm(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let mut addrs = vec![];
    for arg in args {
        let addr =
            Multiaddr::from_str(arg.clone()).map_err(|e| XcliError::BadArgument(e.to_string()))?;
        addrs.push(addr);
    }

    executor::block_on(async {
        for addr in addrs {
            let r = ipfs.remove_listening_address(addr).await;
            println!("removed {:?}", r);
        }
    });

    Ok(CmdExeCode::Ok)
}
