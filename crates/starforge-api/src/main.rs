#[tokio::main]
async fn main() {
    let config = starforge_api::ApiServerConfig::default();
    println!("Starforge API listening on http://{}", config.bind_address);

    if let Err(error) = starforge_api::run_server(config).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
