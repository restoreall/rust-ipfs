use crate::cli::handler;
use crate::MultiaddrWithPeerId;
use futures::executor;
use std::str::FromStr;
use xcli::*;

pub(crate) fn cli_swarm_commands<'a>() -> Command<'a> {
    let connect_swarm_cmd = Command::new_with_alias("connect", "c")
        .about("Open connection to a given address")
        .usage("ipfs swarm connect <address>")
        .action(cli_swarm_connect);

    let disconnect_swarm_cmd = Command::new_with_alias("disconnect", "dc")
        .about("Close connection to a given address")
        .usage("ipfs swarm disconnect <address>")
        .action(cli_swarm_disconnect);

    let addrs_swarm_cmd = Command::new_with_alias("addrs", "a")
        .about("List known addresses. Useful for debugging")
        .usage("ipfs swarm addrs")
        .action(cli_swarm_addrs);

    let peers_swarm_cmd = Command::new_with_alias("peers", "p")
        .about("List peers with open connections")
        .usage("ipfs swarm peers")
        .action(cli_swarm_peers);

    Command::new_with_alias("swarm", "s")
        .about("Manage connections to the p2p network")
        .usage("ipfs swarm")
        .subcommand(addrs_swarm_cmd)
        .subcommand(connect_swarm_cmd)
        .subcommand(disconnect_swarm_cmd)
        .subcommand(peers_swarm_cmd)
}

fn cli_swarm_connect(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let mut ipfs = handler(app);
    let addr = MultiaddrWithPeerId::from_str(args[0])
        .map_err(|e| XcliError::BadArgument(e.to_string()))?;

    executor::block_on(async {
        let r = ipfs.connect(addr).await;
        println!("{:?}", r);
    });

    Ok(CmdExeCode::Ok)
}

fn cli_swarm_disconnect(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let mut ipfs = handler(app);
    let addr = MultiaddrWithPeerId::from_str(args[0])
        .map_err(|e| XcliError::BadArgument(e.to_string()))?;

    executor::block_on(async {
        let r = ipfs.disconnect(addr).await;
        println!("{:?}", r);
    });

    Ok(CmdExeCode::Ok)
}

fn cli_swarm_addrs(app: &App, _args: &[&str]) -> XcliResult {
    let ipfs = handler(app);
    executor::block_on(async {
        let r = ipfs.addrs().await;
        if let Ok(addrs) = r {
            println!("{:?}", addrs);
        }
    });

    Ok(CmdExeCode::Ok)
}

fn cli_swarm_peers(app: &App, _args: &[&str]) -> XcliResult {
    let mut ipfs = handler(app);
    executor::block_on(async {
        let r = ipfs.peers().await;
        if let Ok(peers) = r {
            println!("{:?}", peers);
        }
    });

    Ok(CmdExeCode::Ok)
}
