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
        .subcommand(SubCommand::with_name("query")
                    .about("Dump information about devices connected to the server"))
        .subcommand(SubCommand::with_name("remove")
                    .about("Remove a device from the list. (Note that if the device checks in, it will be added again.)")
                    .arg(Arg::with_name("device")
                         .help("The device to remove")
                         .required(true)
                         .takes_value(true)))
        .subcommand(SubCommand::with_name("create")
                    .about("Create a device in the list.")
                    .arg(Arg::with_name("device")
                         .help("The id of the device to create")
                         .required(true)
                         .takes_value(true)))
        .get_matches();

    // let id = matches.value_of("id").unwrap();
    let addr = matches.value_of("address").unwrap();
    let client = CommsClient::new(&addr);

    match matches.subcommand() {
        ("query", Some(matches)) => query(client, matches),
        ("create", Some(matches)) => create(client, matches),
        ("remove", Some(matches)) => remove(client, matches),
        _ => {},
    }
}

fn query(client: CommsClient, _matches: &clap::ArgMatches) {
    let future = client.get_clients()
        .map(|clients| {
            println!("Received message!");
            println!("{:#?}", clients);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}

fn create(client: CommsClient, matches: &clap::ArgMatches) {
    let device_id = matches.value_of("device").unwrap();
    let future = client.create_device(device_id)
        .map(|response| {
            println!("Received message!");
            println!("{:#?}", response);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}

fn remove(client: CommsClient, matches: &clap::ArgMatches) {
    let device_id = matches.value_of("device").unwrap();
    let future = client.remove_device(device_id)
        .map(|response| {
            println!("Received message!");
            println!("{:#?}", response);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}
