mod cli_bitswap;
mod cli_block;
mod cli_dag;
mod cli_unixfs;

use crate::{Ipfs, TestTypes};
use libp2p_rs::xcli::*;

pub use libp2p_rs::swarm::cli::swarm_cli_commands as scli;

const IPFS: &str = "ipfs";





pub(crate) fn ipfs_cli_commands<'a>() -> Command<'a> {
    Command::new_with_alias("ipfs", "i")
        .about("IPFS")
        .usage("ipfs")
        .subcommand(cli_unixfs::cli_add_commands())
        .subcommand(cli_unixfs::cli_cat_commands())
        .subcommand(cli_unixfs::cli_get_commands())
        .subcommand(cli_block::cli_block_commands())
        .subcommand(cli_dag::cli_dag_commands())
        .subcommand(cli_bitswap::cli_bitswap_commands())
}

pub(crate) fn handler(app: &App) -> Ipfs<TestTypes> {
    let value_any = app.get_handler(IPFS).expect(IPFS);
    let ipfs = value_any
        .downcast_ref::<Ipfs<TestTypes>>()
        .expect("ipfs")
        .clone();
    ipfs
}
