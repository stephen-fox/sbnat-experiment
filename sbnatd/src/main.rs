use std::{error::Error, path::PathBuf, process::exit};

mod server;

fn main() {
    if let Err(err) = main_with_error() {
        eprintln!("fatal: {err}");
        exit(1);
    };
}

fn main_with_error() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    server::Server::block_and_serve(server::Config {
        listen_path: args.socket_path,
    })?;

    Ok(())
}

struct Args {
    socket_path: PathBuf,
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut args = Args {
        socket_path: PathBuf::new(),
    };

    let mut parser = argparse::ArgumentParser::new();

    parser.refer(&mut args.socket_path).add_option(
        &["-s"],
        argparse::Parse,
        "The path of the Unix socket to create",
    );

    parser.parse_args_or_exit();

    drop(parser);

    if args.socket_path.as_os_str().is_empty() {
        Err("please specify a unix socket path to listen on")?
    }

    Ok(args)
}
