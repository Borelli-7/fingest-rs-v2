use fingest_bootstrap::{Config, init_tracing, run};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv::dotenv().ok();

    let config = Config::from_env().unwrap_or_else(|err| {
        eprintln!("configuration error: {err}");
        std::process::exit(1);
    });

    init_tracing(&config.log_level);

    run(config).await.map_err(std::io::Error::other)
}
