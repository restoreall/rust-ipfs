use crate::cli::handler;
use futures::{executor, pin_mut, stream::StreamExt};
use ipfs_unixfs::file::adder::FileAdder;
use xcli::*;
use std::convert::TryFrom;
use crate::{IpfsPath, Block};
use std::process::exit;

pub(crate) fn cli_add_commands<'a>() -> Command<'a> {
    Command::new_with_alias("add", "a")
        .about("Add a file to IPFS")
        .usage("ipfs add <path>...")
        .action(cli_add)
}

fn cli_add(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);

    let mut adder = FileAdder::default();
    let _ = adder.push(args[0].as_bytes());

    executor::block_on(async {
        for (cid, data) in adder.finish() {
            let len = data.len();
            let block = Block {
                cid: cid.clone(),
                data: data.into(),
            };

            if ipfs.put_block(block).await.is_ok() {
                println!("added {} {}B", cid, len);
            }
        }
    });

    Ok(CmdExeCode::Ok)
}

pub(crate) fn cli_cat_commands<'a>() -> Command<'a> {
    Command::new_with_alias("cat", "c")
        .about("Show IPFS object data")
        .usage("ipfs cat <ipfs-path>...")
        .action(cli_cat)
}

fn cli_cat(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let path = IpfsPath::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    executor::block_on(async {
        let stream = ipfs.cat_unixfs(path, None).await.unwrap_or_else(|e| {
            eprintln!("Error: {:?}", e);
            exit(1);
        });

        // The stream needs to be pinned on the stack to be used with StreamExt::next
        pin_mut!(stream);

        loop {
            // This could be made more performant by polling the stream while writing to stdout.
            match stream.next().await {
                Some(Ok(bytes)) => {
                    println!("{:?}", bytes);
                }
                Some(Err(e)) => {
                    eprintln!("Error: {}", e);
                    exit(1);
                }
                None => break,
            }
        }
    });

    Ok(CmdExeCode::Ok)
}

pub(crate) fn cli_get_commands<'a>() -> Command<'a> {
    Command::new_with_alias("get", "g")
        .about("Download IPFS objects")
        .usage("ipfs get <ipfs-path>")
        .action(cli_get)
}

fn cli_get(app: &App, args: &[&str]) -> XcliResult {
    if args.len() < 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let path = IpfsPath::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    executor::block_on(async {
        let stream = ipfs.cat_unixfs(path, None).await.unwrap_or_else(|e| {
            eprintln!("Error: {:?}", e);
            exit(1);
        });

        // The stream needs to be pinned on the stack to be used with StreamExt::next
        pin_mut!(stream);

        loop {
            // This could be made more performant by polling the stream while writing to stdout.
            match stream.next().await {
                Some(Ok(bytes)) => {
                    println!("{:?}", bytes);
                }
                Some(Err(e)) => {
                    eprintln!("Error: {}", e);
                    exit(1);
                }
                None => break,
            }
        }
    });

    Ok(CmdExeCode::Ok)
}