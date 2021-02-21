use futures::executor;
use libp2p_rs::core::PeerId;
use std::str::FromStr;
use libp2p_rs::xcli::*;
use bitswap::Control;

const BITSWAP: &str = "bitswap";

pub(crate) fn bitswap_cli_commands<'a>() -> Command<'a> {
    let wl_cmd = Command::new_with_alias("wantlist", "w")
        .about("Show blocks currently on the wantlist")
        .usage("ipfs bitswap wantlist")
        .action(cli_wl_bitswap);
    let stat_cmd = Command::new_with_alias("stat", "s")
        .about("Show some diagnostic information on the bitswap agent")
        .usage("ipfs bitswap stat")
        .action(cli_stat_bitswap);

    Command::new_with_alias(BITSWAP, "bs")
        .about("exchange blocks with other peers")
        .usage("bitswap")
        .subcommand(stat_cmd)
        .subcommand(wl_cmd)
}

pub(crate) fn handler(app: &App) -> Control {
    let value_any = app.get_handler(BITSWAP).expect(BITSWAP);
    let ipfs = value_any
        .downcast_ref::<Control>()
        .expect("ipfs")
        .clone();
    ipfs
}


fn cli_wl_bitswap(app: &App, args: &[&str]) -> XcliResult {
    let mut peer = None;
    if args.len() == 1 {
        let p = PeerId::from_str(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?;
        peer = Some(p);
    }

    let mut bitswap = handler(app);
    executor::block_on(async {
        bitswap.wantlist(peer)
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
    let mut bitswap = handler(app);
    executor::block_on(async {
        let r = bitswap.stats().await;
        if let Ok(st) = r {
            println!("{:?}", st);
        }
    });

    Ok(CmdExeCode::Ok)
}
