mod cli_block;
mod cli_dag;
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
}

pub(crate) fn handler(app: &App) -> Ipfs<TestTypes>
{
    let value_any = app.get_handler(IPFS).expect(IPFS);
    let ipfs = value_any.downcast_ref::<Ipfs<TestTypes>>().expect("ipfs").clone();
    ipfs
}
