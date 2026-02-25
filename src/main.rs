use crunchyroll_downloader_telegram_bot::config::Config;
use crunchyroll_downloader_telegram_bot::telegram::run_bot;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use std::panic;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create logs directory
    let logs_dir = std::path::Path::new("logs");
    std::fs::create_dir_all(logs_dir).ok();

    // Set up file appender with daily rotation
    let file_appender = tracing_appender::rolling::daily(logs_dir, "analime-bot.log");
    let (non_blocking_file, _guard) = tracing_appender::non_blocking(file_appender);

    // Set up logging to both console and file
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // Console layer - human readable
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false);

    // File layer - more detailed with timestamps
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false)  // No color codes in file
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    // Set up panic hook to log panics before crashing
    let default_panic = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let location = panic_info.location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic message".to_string()
        };

        tracing::error!("🔥 PANIC at {}: {}", location, message);
        tracing::error!("Backtrace:\n{:?}", std::backtrace::Backtrace::capture());

        // Call the default panic handler
        default_panic(panic_info);
    }));

    tracing::info!("========================================");
    tracing::info!("AnAlime Bot starting...");
    tracing::info!("Log file: logs/analime-bot.YYYY-MM-DD.log");
    tracing::info!("========================================");

    // Load configuration
    let config = match Config::load_or_default() {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("Failed to load configuration: {}", e);
            tracing::info!("Please create a config.toml file with the following structure:");
            print_example_config();
            return Err(e.into());
        }
    };

    tracing::info!("Configuration loaded successfully");

    // Create download directories if they don't exist
    std::fs::create_dir_all(&config.download.temp_dir).ok();
    std::fs::create_dir_all(&config.download.output_dir).ok();

    // Run the bot
    if let Err(e) = run_bot(config).await {
        tracing::error!("Bot error: {}", e);
        return Err(e.into());
    }

    Ok(())
}

fn print_example_config() {
    let example = r#"
[telegram]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"
allowed_users = []  # Empty array allows all users, or add user IDs

[crunchyroll]
email = "your@email.com"
password = "your_password"
locale = "en-US"
preferred_audio = ["ja-JP", "en-US"]

[download]
temp_dir = "./temp"
output_dir = "./downloads"
max_concurrent_segments = 8

[widevine]
client_id_path = "src/device/client_id.bin"
private_key_path = "src/device/private_key.pem"
"#;
    println!("{}", example);
}
