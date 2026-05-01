use teloxide::utils::command::BotCommands;
use crate::config::OwnerUser;
use crate::i18n::Strings;

/// Bot commands
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "📋 Commands:")]
pub enum Command {
    #[command(description = "🚀 Start the bot")]
    Start,

    #[command(description = "🔍 Search anime by name")]
    Search,

    #[command(description = "📥 Download by Crunchyroll ID or URL")]
    Get,

    #[command(description = "❓ Show help")]
    Help,

    #[command(description = "❌ Cancel current operation")]
    Cancel,

    #[command(description = "📊 Bot status")]
    Status,

    #[command(description = "📈 Bot statistics")]
    Stats,

    #[command(description = "💝 Support the bot")]
    Donate,

    #[command(description = "🔧 Check tools (mp4decrypt, FFmpeg)")]
    Tools,

    #[command(description = "⬇️ Auto-download missing tools")]
    InstallTools,

    #[command(description = "👑 Add admin (owner only)")]
    AddAdmin,

    #[command(description = "🚫 Remove admin (owner only)")]
    RemoveAdmin,

    #[command(description = "✅ Authorize a chat/user")]
    Authorize,

    #[command(description = "❌ Revoke authorization")]
    Deauthorize,
}

/// Content type detected from ID or URL
#[derive(Debug, Clone)]
pub enum ContentType {
    Series(String),
    Season(String),
    Episode(String),
    MovieListing(String),
    Movie(String),
}

/// Parse a Crunchyroll URL or ID to extract content information
pub fn parse_crunchyroll_input(input: &str) -> Option<ContentType> {
    let input = input.trim();

    if input.starts_with("http://") || input.starts_with("https://") {
        return parse_crunchyroll_url(input);
    }

    if input.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Some(ContentType::Episode(input.to_string()));
    }

    None
}

fn parse_crunchyroll_url(url: &str) -> Option<ContentType> {
    let path = if let Some(pos) = url.find("crunchyroll.com") {
        &url[pos + 15..]
    } else {
        return None;
    };

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }

    match segments.get(0).map(|s| *s) {
        Some("series") => segments.get(1).map(|id| ContentType::Series(id.to_string())),
        Some("watch") => segments.get(1).map(|id| ContentType::Episode(id.to_string())),
        Some(segment) if segment.starts_with("G") || segment.starts_with("g") => {
            Some(ContentType::Episode(segment.to_string()))
        }
        _ => None,
    }
}

fn owner_info_line(owners: &[OwnerUser], strings: &Strings) -> String {
    if owners.is_empty() {
        String::new()
    } else {
        let names = owners.iter().map(|o| o.display()).collect::<Vec<_>>().join(", ");
        format!("\n\n{} {}", strings.owner_label, names)
    }
}

fn escape_md(s: &str) -> String {
    s.replace('.', "\\.").replace('!', "\\!").replace('-', "\\-")
     .replace('(', "\\(").replace(')', "\\)")
}

pub fn build_not_authorized_message(owners: &[OwnerUser], strings: &Strings) -> String {
    let contacts: Vec<String> = owners.iter().filter_map(|o| o.username.as_ref().cloned()).collect();
    if contacts.is_empty() {
        strings.not_authorized_msg.to_string()
    } else {
        format!(
            "{}{}",
            strings.not_authorized_msg,
            strings.not_authorized_contact.replace("{owners}", &contacts.join(", "))
        )
    }
}

pub fn build_callback_not_authorized(owners: &[OwnerUser], strings: &Strings) -> String {
    let contacts: Vec<String> = owners.iter().filter_map(|o| o.username.as_ref().cloned()).collect();
    if contacts.is_empty() {
        strings.not_authorized_callback.to_string()
    } else {
        strings.not_authorized_callback_contact.replace("{owners}", &contacts.join(", "))
    }
}

pub fn build_welcome_message(owners: &[OwnerUser], bot_name: &str, strings: &Strings) -> String {
    format!(
        "🎬 *{}\\!*\n\n{}{}",
        escape_md(bot_name),
        strings.welcome_commands,
        owner_info_line(owners, strings),
    )
}

pub fn build_help_message(owners: &[OwnerUser], bot_name: &str, strings: &Strings) -> String {
    format!(
        "📖 *{}*\n\n{}{}",
        escape_md(bot_name),
        strings.help_body,
        owner_info_line(owners, strings),
    )
}

pub fn build_donate_message(bot_name: &str, strings: &Strings) -> String {
    format!(
        "💝 *{}*\n\n{}",
        escape_md(bot_name),
        strings.donate_body,
    )
}
