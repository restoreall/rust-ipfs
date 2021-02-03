mod cli_bitswap;
mod cli_block;
mod cli_bootstrap;
mod cli_dag;
mod cli_dht;
mod cli_listen;
mod cli_swarm;
mod cli_unixfs;

use crate::{Ipfs, TestTypes};
use xcli::*;

const IPFS: &str = "ipfs";

pub fn ipfs_cli_commands<'a>() -> Command<'a> {
    Command::new_with_alias("ipfs", "i")
        .about("IPFS")
        .usage("ipfs")
        .subcommand(cli_unixfs::cli_add_commands())
        .subcommand(cli_unixfs::cli_cat_commands())
        .subcommand(cli_block::cli_block_commands())
        .subcommand(cli_dag::cli_dag_commands())
        .subcommand(cli_swarm::cli_swarm_commands())
        .subcommand(cli_bitswap::cli_bitswap_commands())
        .subcommand(cli_dht::cli_dht_commands())
        .subcommand(cli_bootstrap::cli_bootstrap_commands())
        .subcommand(cli_listen::cli_listen_commands())
}

pub(crate) fn handler(app: &App) -> Ipfs<TestTypes> {
    let value_any = app.get_handler(IPFS).expect(IPFS);
    let ipfs = value_any
        .downcast_ref::<Ipfs<TestTypes>>()
        .expect("ipfs")
        .clone();
    ipfs
}
