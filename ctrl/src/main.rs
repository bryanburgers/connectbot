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
        .subcommand(SubCommand::with_name("connect")
                    .about("Create an SSH connection")
                    .arg(Arg::with_name("device")
                         .help("The id of the device to connect")
                         .required(true)
                         .takes_value(true))
                    .arg(Arg::with_name("port")
                         .short("p")
                         .long("port")
                         .validator(|item| {
                             match item.parse::<i32>() {
                                 Ok(0) => Err("Port must be a valid port number".to_string()),
                                 Ok(_) => Ok(()),
                                 Err(_) => Err("Port must be a valid port number".to_string()),
                             }
                         })
                         .help("The local port to forward")
                         .required(true)
                         .takes_value(true)))
        .subcommand(SubCommand::with_name("disconnect")
                    .about("Disconnect an SSH connection")
                    .arg(Arg::with_name("device")
                         .help("The id of the device to connect")
                         .required(true)
                         .takes_value(true))
                    .arg(Arg::with_name("connection-id")
                         .long("connection-id")
                         .help("The connection ID of the SSH connection to disconnect")
                         .required(true)
                         .takes_value(true)))
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
        .subcommand(SubCommand::with_name("set-name")
                    .about("Change the human-friendly name of a device")
                    .arg(Arg::with_name("device")
                         .help("The id of the device to change")
                         .required(true)
                         .takes_value(true))
                    .arg(Arg::with_name("name")
                         .help("The new name of the device")
                         .required(true)
                         .takes_value(true)))
        .get_matches();

    // let id = matches.value_of("id").unwrap();
    let addr = matches.value_of("address").unwrap();
    let client = CommsClient::new(&addr);

    match matches.subcommand() {
        ("connect", Some(matches)) => connect(client, matches),
        ("disconnect", Some(matches)) => disconnect(client, matches),
        ("query", Some(matches)) => query(client, matches),
        ("create", Some(matches)) => create(client, matches),
        ("remove", Some(matches)) => remove(client, matches),
        ("set-name", Some(matches)) => set_name(client, matches),
        _ => {},
    }
}

fn connect(client: CommsClient, matches: &clap::ArgMatches) {
    let device_id = matches.value_of("device").unwrap();
    let port = matches.value_of("port").unwrap().parse().unwrap();
    let future = client.connect_device(device_id, port)
        .map(|response| {
            println!("{:#?}", response);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}

fn disconnect(client: CommsClient, matches: &clap::ArgMatches) {
    let device_id = matches.value_of("device").unwrap();
    let connection_id = matches.value_of("connection-id").unwrap();
    let future = client.disconnect_connection(device_id, connection_id)
        .map(|response| {
            println!("{:#?}", response);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}

fn query(client: CommsClient, _matches: &clap::ArgMatches) {
    let future = client.get_clients()
        .map(|clients| {
            println!("{:#?}", clients);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}

fn create(client: CommsClient, matches: &clap::ArgMatches) {
    let device_id = matches.value_of("device").unwrap();
    let future = client.create_device(device_id)
        .map(|response| {
            println!("{:#?}", response);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}

fn remove(client: CommsClient, matches: &clap::ArgMatches) {
    let device_id = matches.value_of("device").unwrap();
    let future = client.remove_device(device_id)
        .map(|response| {
            println!("{:#?}", response);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}

fn set_name(client: CommsClient, matches: &clap::ArgMatches) {
    let device_id = matches.value_of("device").unwrap();
    let name = matches.value_of("name").unwrap();
    let future = client.set_name(device_id, name)
        .map(|response| {
            println!("{:#?}", response);
        })
        .map_err(|e| println!("Error: {}", e));

    tokio::run(future);
}
