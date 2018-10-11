extern crate clap;
extern crate tokio;
extern crate futures;
extern crate tokio_codec;

extern crate connectbot_shared;

use clap::{App, AppSettings, Arg, SubCommand};
use futures::Future;
use connectbot_shared::client::Client as CommsClient;

fn main() {
    let matches = App::new("connectbot-ctrl")
        .version("1.0")
        .author("Bryan Burgers <bryan@burgers.io>")
        .about("Communications")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(Arg::with_name("address")
             .short("a")
             .long("address")
             .help("The address to use to communicate with the lights daemon")
             .takes_value(true)
             .default_value("[::1]:12345"))
        .subcommand(SubCommand::with_name("connect"))
        .subcommand(SubCommand::with_name("disconnect"))
        .subcommand(SubCommand::with_name("query"))
        .get_matches();

    // let id = matches.value_of("id").unwrap();
    let addr = matches.value_of("address").unwrap();

    let client = CommsClient::new(&addr);
    let future = client.get_clients()
        .map(|clients| {
            println!("Received message!");
            println!("{:#?}", clients);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}
