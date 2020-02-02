extern crate chrono;
extern crate clap;
extern crate connectbot_shared;
extern crate futures;
extern crate handlebars;
extern crate http;
extern crate serde_derive;
extern crate tokio;
extern crate toml;
extern crate tower_web;

use clap::{App, AppSettings, Arg, SubCommand};
use std::path::Path;
use tower_web::view::Handlebars;
use tower_web::ServiceBuilder;

mod config;
mod service;

pub fn main() {
    let matches = App::new("connectbot-web")
        .version("1.0")
        .author("Bryan Burgers <bryan@burgers.io>")
        .setting(AppSettings::SubcommandsNegateReqs)
        .subcommand(SubCommand::with_name("config").about("Generate an example config file"))
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .help("The location of the config file")
                .takes_value(true)
                .default_value("/etc/connectbot/web.conf"),
        )
        .get_matches();

    if let Some(_matches) = matches.subcommand_matches("config") {
        let config: config::ApplicationConfig = std::default::Default::default();
        print!("{}", toml::to_string(&config).unwrap());
        return;
    }

    let config_file = Path::new(matches.value_of_os("config").unwrap());
    let config_base = config_file.parent().unwrap();

    let result = config::ApplicationConfig::from_file(config_file);
    let config = match result {
        Ok(config) => config,
        Err(string) => {
            println!("{}", string);
            std::process::exit(1);
        }
    };

    let address = (&config.address)
        .parse()
        .expect("address must be a valid socket address");
    let control_address = &config.control_address;

    println!("Listening on http://{}", address);

    let templates_path = config_base.join(config.templates.path);

    let mut handlebars_registry = handlebars::Handlebars::new();
    handlebars_registry
        .register_templates_directory(".hbs", templates_path)
        .expect("templates path must exist");

    ServiceBuilder::new()
        .resource(service::ConnectBotWeb::new(control_address))
        .serializer(Handlebars::new_with_registry(handlebars_registry))
        .run(&address)
        .unwrap();
}
