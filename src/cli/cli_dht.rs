use xcli::*;

pub(crate) fn cli_dht_commands<'a>() -> Command<'a> {
    Command::new_with_alias("dht", "d")
        .about("Query the DHT for values or peers")
        .usage("ipfs dht")
}
