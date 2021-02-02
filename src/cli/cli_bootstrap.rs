use crate::cli::handler;
use crate::MultiaddrWithPeerId;
use futures::executor;
use std::str::FromStr;
use xcli::*;

pub(crate) fn cli_bootstrap_commands<'a>() -> Command<'a> {
    let add_boot_cmd = Command::new_with_alias("add", "a")
        .about("Add peers to the bootstrap list.")
        .usage("ipfs bootstrap add [<peer>]...")
        .action(cli_boot_add);
    let list_boot_cmd = Command::new_with_alias("list", "l")
        .about("Show peers in the bootstrap list.")
        .usage("ipfs bootstrap list")
        .action(cli_boot_list);
    let rm_boot_cmd = Command::new_with_alias("rm", "r")
        .about("Remove peers from the bootstrap list.")
        .usage("ipfs bootstrap rm [<peer>]...")
        .action(cli_boot_rm);

    Command::new_with_alias("bootstrap", "boot")
        .about("Add or remove bootstrap peers")
        .usage("ipfs bootstrap - Show or edit the list of bootstrap peers.")
        .subcommand(add_boot_cmd)
        .subcommand(list_boot_cmd)
        .subcommand(rm_boot_cmd)
}

fn cli_boot_add(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let mut addrs = vec![];
    for arg in args {
        let addr = MultiaddrWithPeerId::from_str(arg.clone())
            .map_err(|e| XcliError::BadArgument(e.to_string()))?;
        addrs.push(addr);
    }

    executor::block_on(async {
        for addr in addrs {
            let r = ipfs.add_bootstrapper(addr).await;
            println!("added {:?}", r);
        }
    });

    Ok(CmdExeCode::Ok)
}

fn cli_boot_list(app: &App, _args: &[&str]) -> XcliResult {
    let ipfs = handler(app);

    executor::block_on(async {
        ipfs.get_bootstrappers()
            .await
            .map_or(println!("none"), |addrs| {
                addrs.iter().for_each(|a| println!("{:?}", a))
            });
    });

    Ok(CmdExeCode::Ok)
}

fn cli_boot_rm(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let mut addrs = vec![];
    for arg in args {
        let addr = MultiaddrWithPeerId::from_str(arg.clone())
            .map_err(|e| XcliError::BadArgument(e.to_string()))?;
        addrs.push(addr);
    }

    executor::block_on(async {
        for addr in addrs {
            let r = ipfs.remove_bootstrapper(addr).await;
            println!("removed {:?}", r);
        }
    });

    Ok(CmdExeCode::Ok)
}
