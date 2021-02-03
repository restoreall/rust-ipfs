use crate::cli::handler;
use futures::executor;
use libp2p::PeerId;
use std::str::FromStr;
use xcli::*;

pub(crate) fn cli_bitswap_commands<'a>() -> Command<'a> {
    let wl_cmd = Command::new_with_alias("wantlist", "w")
        .about("Show blocks currently on the wantlist")
        .usage("ipfs bitswap wantlist")
        .action(cli_wl_bitswap);
    let stat_cmd = Command::new_with_alias("stat", "s")
        .about("Show some diagnostic information on the bitswap agent")
        .usage("ipfs bitswap stat")
        .action(cli_stat_bitswap);

    Command::new_with_alias("bitswap", "bs")
        .about("exchange block with other peer")
        .usage("ipfs bitswap")
        .subcommand(stat_cmd)
        .subcommand(wl_cmd)
}

fn cli_wl_bitswap(app: &App, args: &[&str]) -> XcliResult {
    let mut peer = None;
    if args.len() == 1 {
        let p = PeerId::from_str(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?;
        peer = Some(p);
    }

    let ipfs = handler(app);
    executor::block_on(async {
        ipfs.bitswap_wantlist(peer)
            .await
            .map_or(println!("none"), |addrs| {
                addrs
                    .iter()
                    .for_each(|(c, p)| println!("Cid: {} Priority: {:?}", c, p))
            });
    });

    Ok(CmdExeCode::Ok)
}

fn cli_stat_bitswap(app: &App, _args: &[&str]) -> XcliResult {
    let ipfs = handler(app);
    executor::block_on(async {
        let r = ipfs.bitswap_stats().await;
        if let Ok(st) = r {
            println!("{:?}", st);
        }
    });

    Ok(CmdExeCode::Ok)
}
