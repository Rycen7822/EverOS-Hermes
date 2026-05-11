fn main() {
    if let Err(err) = everos_hermes_rust::cli::run() {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
