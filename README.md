# Crunchyroll Downloader Telegram Bot

[![Release](https://img.shields.io/github/v/release/AnCry1596/Crunchyroll-Downloader-Bot)](https://github.com/AnCry1596/Crunchyroll-Downloader-Bot/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/AnCry1596/Crunchyroll-Downloader-Bot/release.yml)](https://github.com/AnCry1596/Crunchyroll-Downloader-Bot/actions)
[![License: WTFPL](https://img.shields.io/badge/license-WTFPL-brightgreen)](LICENSE)

A Telegram bot written in Rust that downloads Crunchyroll anime episodes with Widevine DRM decryption and uploads them to Telegram or external file hosting services.

## Features

- Download Crunchyroll anime episodes via Telegram commands
- Widevine DRM decryption (DASH streams)
- Automatic upload to Telegram (up to ~2GB) or external services for larger files
- External upload services: [Buzzheavier](https://buzzheavier.com), [Pixeldrain](https://pixeldrain.com), [Gofile](https://gofile.io)
- Multi-language audio support with preferred audio track selection
- MongoDB-backed caching to avoid re-uploading the same file twice
- Proxy support (HTTP, SOCKS4/5) for geo-restricted content
- Owner/admin permission system with per-chat authorization

## Download

Pre-built binaries are available on the [Releases](https://github.com/AnCry1596/Crunchyroll-Downloader-Bot/releases/latest) page:

| Platform | Binary |
|---|---|
| Linux x86_64 | `crunchyroll-downloader-bot-linux-x86_64` |
| Windows x86_64 | `crunchyroll-downloader-bot-windows-x86_64.exe` |

## Requirements

- [MongoDB](https://www.mongodb.com/) instance
- A Telegram bot token (from [@BotFather](https://t.me/BotFather))
- A Crunchyroll account
- Widevine device credentials (`client_id.bin` + `private_key.pem`)
- [ffmpeg](https://ffmpeg.org/) in PATH (for muxing)
- [mp4decrypt](https://www.bento4.com/) in PATH (for DRM decryption)

## Setup

1. Download the binary for your platform from [Releases](https://github.com/AnCry1596/Crunchyroll-Downloader-Bot/releases/latest), or build from source:
   ```bash
   git clone https://github.com/AnCry1596/Crunchyroll-Downloader-Bot.git
   cd Crunchyroll-Downloader-Bot
   cargo build --release
   ```

2. Download `config.example.toml` from the release and copy it:
   ```bash
   cp config.example.toml config.toml
   ```

3. Edit `config.toml`:
   - Set your Telegram `bot_token` and `owner_users`
   - Set your Crunchyroll `email` and `password`
   - Set your MongoDB `connection_string` and `db_name`
   - Place your Widevine `client_id.bin` and `private_key.pem` and set the paths
   - (Optional) Set `language = "en"` for English UI (default: `"vi"`)
   - (Optional) Add API keys for Pixeldrain, Buzzheavier, or Gofile
   - (Optional) Configure proxies for geo-restricted regions

4. Run:
   ```bash
   # Linux
   chmod +x crunchyroll-downloader-bot-linux-x86_64
   ./crunchyroll-downloader-bot-linux-x86_64

   # Windows
   crunchyroll-downloader-bot-windows-x86_64.exe
   ```

## Configuration

See [config.example.toml](config.example.toml) for all available options with descriptions.

| Section | Key | Description |
|---|---|---|
| `[telegram]` | `bot_token` | Bot token from @BotFather |
| `[telegram]` | `owner_users` | List of owner user IDs |
| `[telegram]` | `storage_chat_id` | Group/channel ID for file caching |
| `[telegram]` | `language` | UI language: `"vi"` (default) or `"en"` |
| `[crunchyroll]` | `email` / `password` | Crunchyroll credentials |
| `[crunchyroll]` | `preferred_audio` | Preferred audio languages in order |
| `[download]` | `upload_preference` | `"telegram"` or `"service"` |
| `[download]` | `preferred_upload_service` | `"buzzheavier"`, `"pixeldrain"`, or `"gofile"` |
| `[widevine]` | `client_id_path` | Path to Widevine `client_id.bin` |
| `[widevine]` | `private_key_path` | Path to Widevine `private_key.pem` |
| `[database]` | `connection_string` | MongoDB connection URI |
| `[proxy]` | `main_proxy` | Global proxy (optional) |

## Acknowledgements

Special thanks to [multi-downloader-nx](https://github.com/anidl/multi-downloader-nx) for the Crunchyroll API implementation that made this project possible.

## License

[WTFPL](LICENSE) — Do What The Fuck You Want To Public License
