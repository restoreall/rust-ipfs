use crate::cli::handler;
use crate::unixfs::ll::walk::{ContinuedWalk, Walker};
use crate::Cid;
use crate::{Block, IpfsPath};
use futures::{executor, pin_mut, stream::StreamExt};
use ipfs_unixfs::file::adder::FileAdder;
use std::convert::TryFrom;
use std::fs::{DirBuilder, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::exit;
use libp2p_rs::xcli::*;
use libp2p_rs::xcli::XcliError::BadArgument;

pub(crate) fn cli_add_commands<'a>() -> Command<'a> {
    Command::new_with_alias("add", "a")
        .about("Add a file to IPFS")
        .usage("ipfs add <path>...")
        .action(cli_add)
}

fn cli_add(app: &App, args: &[&str]) -> XcliResult {
    if args.is_empty() {
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
    if args.is_empty() {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);
    let path = IpfsPath::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    executor::block_on(async {
        let stream = ipfs.cat_unixfs(path, None).await.unwrap_or_else(|e| {
            println!("Error: {:?}", e);
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
                    println!("Error: {:?}", e);
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
    let mut cache = None;
    let mut root = PathBuf::from("./");

    // A symbol that means in the same tree.
    let mut same_file = false;

    if args.is_empty() {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let ipfs = handler(app);

    let cid = Cid::try_from(args[0]).map_err(|e| {
       BadArgument(e.to_string())
    })?;

    let mut walker = Walker::new(cid, "".to_string());

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        while walker.should_continue() {
            let (cid, _) = walker.pending_links();

            let tmp_cid = cid.clone();

            println!("fetching cid {}...", cid);

            let ipld_block = ipfs.get_block(cid).await.unwrap_or_else(|e| {
                println!("Failed to get block {}: {:?}", cid, e);
                exit(1);
            });

            println!("fetching cid {} done", cid);

            match walker
                .next(&ipld_block.into_vec(), &mut cache)
                .unwrap_or_else(|e| {
                    println!("Error in walker.next(): {:?}", e);
                    exit(1);
                })
            {
                ContinuedWalk::Bucket(..) => {
                    // Continuation of a HAMT shard directory that is usually ignored
                }
                ContinuedWalk::File(segment, _, path, _, _) => {
                    let mut file_name = PathBuf::new();

                    if let Some("./") = root.to_str() {
                        if let Some("") = path.to_str() {
                            root.push(multibase::Base::Base32Upper.encode(tmp_cid.to_bytes()));
                            file_name = root.clone();
                        }
                    } else {
                        file_name = root.clone();
                        if Some("") != path.to_str() {
                            file_name.push(path);
                        }
                    }

                    // If true, means that it is the first block.
                    if segment.is_first() {
                        same_file = true;
                        let _ = OpenOptions::new().write(true).append(true)
                            .create(true).open(file_name.clone()).unwrap_or_else(|e| {
                            println!("Error in create file: {:?}", e);
                            exit(1);
                        });
                    }

                    // If true, it means file is larger than 256k and has been split
                    if same_file {
                        let mut file = OpenOptions::new().append(true).open(file_name.clone())
                            .map_err(|e| {
                                println!("Error in open file: {:?}", e);
                                exit(1);
                            })
                            .unwrap();

                        file.write_all(segment.as_bytes()).unwrap_or_else(|e| {
                            println!("Error in write_all: {:?}", e);
                            exit(1);
                        });
                    }

                    if segment.is_last() {
                        same_file = false;
                        println!("filename: {:?}", file_name);
                    }
                }
                ContinuedWalk::Directory(_, path, _metadata)
                | ContinuedWalk::RootDirectory(_, path, _metadata) => {
                    // Root directory
                    if let Some("") = path.to_str() {
                        root.push(multibase::Base::Base32Upper.encode(tmp_cid.to_bytes()));
                        DirBuilder::new().create(&root)
                            .unwrap_or_else(|e| {
                                println!("Error in create dir by path empty: {:?}", e);
                                exit(1);
                            });
                        println!("Directory: {:?}", &root)
                    } else {
                        // Other directory
                        let mut root_tmp = root.clone();
                        root_tmp.push(path);
                        DirBuilder::new().create(&root_tmp)
                            .unwrap_or_else(|e| {
                                println!("Error in create dir: {:?}", e);
                                exit(1);
                            });
                        println!("Directory: {:?}", root_tmp)
                    }
                }
                ContinuedWalk::Symlink(_, _, _, _) => {}
            }
        }
    });

    Ok(CmdExeCode::Ok)
}