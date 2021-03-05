use crate::cli::handler;
use cid::Cid;
use libp2p_rs::runtime::task;
use std::convert::TryFrom;
use libp2p_rs::xcli::*;

pub(crate) fn cli_dag_commands<'a>() -> Command<'a> {
    let get_dag_cmd = Command::new_with_alias("get", "g")
        .about("Get a dag node from ipfs")
        .usage("get <ref>")
        .action(cli_dag_get);
    let put_dag_cmd = Command::new_with_alias("put", "p")
        .about("Add a dag node to ipfs")
        .usage("put <string>")
        .action(cli_dag_put);

    Command::new_with_alias("dag", "d")
        .about("Interact with IPLD documents (experimental)")
        .usage("ipfs dag")
        .subcommand(get_dag_cmd)
        .subcommand(put_dag_cmd)
}

fn cli_dag_get(app: &App, args: &[&str]) -> XcliResult {
    if args.is_empty() {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let cid = Cid::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    task::block_on(async {
        let r = ipfs.get_dag(cid.into()).await;
        if let Ok(data) = r {
            println!("{:?}", data);
        }
    });

    Ok(CmdExeCode::Ok)
}

fn cli_dag_put(app: &App, args: &[&str]) -> XcliResult {
    if args.is_empty() {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let data = make_ipld!(args[0].as_bytes());

    task::block_on(async {
        let r = ipfs.put_dag(data).await;
        if let Ok(cid) = r {
            println!("{} {:?}", cid, args[0]);
        }
    });

    Ok(CmdExeCode::Ok)
}
