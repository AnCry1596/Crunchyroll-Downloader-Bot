use teloxide::utils::command::BotCommands;

/// Bot commands
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "📋 Danh sách lệnh:")]
pub enum Command {
    #[command(description = "🚀 Khởi động bot")]
    Start,

    #[command(description = "🔍 Tìm kiếm anime theo tên")]
    Search,

    // #[command(description = "📥 Tải nội dung bằng ID hoặc URL")]
    // Get,

    #[command(description = "❓ Hiển thị hướng dẫn")]
    Help,

    #[command(description = "❌ Huỷ thao tác hiện tại")]
    Cancel,

    #[command(description = "📊 Xem trạng thái bot")]
    Status,

    #[command(description = "📈 Xem thống kê bot")]
    Stats,

    #[command(description = "💝 Ủng hộ bot")]
    Donate,

    #[command(description = "🔧 Kiểm tra công cụ (mp4decrypt, FFmpeg)")]
    Tools,

    #[command(description = "⬇️ Tự động tải công cụ còn thiếu")]
    InstallTools,

    #[command(description = "👑 Thêm admin (owner only)")]
    AddAdmin,

    #[command(description = "🚫 Xoá admin (owner only)")]
    RemoveAdmin,

    #[command(description = "✅ Cấp quyền sử dụng cho chat/user")]
    Authorize,

    #[command(description = "❌ Thu hồi quyền sử dụng")]
    Deauthorize,
}

impl Command {
    pub fn description_for(&self) -> &'static str {
        match self {
            Command::Start => "🚀 Chào mừng! Dùng /search để tìm anime.",
            Command::Search => "🔍 Tìm kiếm anime. Cú pháp: /search <tên anime>",
            // Command::Get => "📥 Tải bằng ID hoặc URL. Cú pháp: /get <id hoặc url>",
            Command::Help => "❓ Hiển thị hướng dẫn sử dụng.",
            Command::Cancel => "❌ Huỷ thao tác đang thực hiện.",
            Command::Status => "📊 Hiển thị trạng thái bot.",
            Command::Stats => "📈 Hiển thị thống kê bot.",
            Command::Donate => "💝 Hiển thị thông tin ủng hộ.",
            Command::Tools => "🔧 Kiểm tra trạng thái công cụ.",
            Command::InstallTools => "⬇️ Tải và cài đặt công cụ còn thiếu.",
            Command::AddAdmin => "👑 Thêm admin. Cú pháp: /addadmin <user_id> hoặc reply tin nhắn.",
            Command::RemoveAdmin => "🚫 Xoá admin. Cú pháp: /removeadmin <user_id> hoặc reply tin nhắn.",
            Command::Authorize => "✅ Cấp quyền. Cú pháp: /authorize hoặc /authorize <chat_id>.",
            Command::Deauthorize => "❌ Thu hồi quyền. Cú pháp: /deauthorize hoặc /deauthorize <chat_id>.",
        }
    }
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

    // Check if it's a URL
    if input.starts_with("http://") || input.starts_with("https://") {
        return parse_crunchyroll_url(input);
    }

    // Check if it's a raw ID (alphanumeric with possible hyphens/underscores)
    if input.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        // Try to determine ID type by prefix patterns
        // Crunchyroll IDs typically follow patterns like:
        // G... for series/movies, GY... for seasons, GZ... for episodes
        let upper = input.to_uppercase();
        if upper.starts_with("G") {
            // Could be series, movie, season, or episode
            // Return as generic - will be resolved by trying different endpoints
            return Some(ContentType::Episode(input.to_string()));
        }
        // If no clear pattern, treat as episode ID (most common use case)
        return Some(ContentType::Episode(input.to_string()));
    }

    None
}

/// Parse a Crunchyroll URL to extract content type and ID
fn parse_crunchyroll_url(url: &str) -> Option<ContentType> {
    // Examples:
    // https://www.crunchyroll.com/series/GRMG8ZQZR/one-piece
    // https://www.crunchyroll.com/watch/GVWU0XK4Y/romance-dawn
    // https://www.crunchyroll.com/watch/G4PH0WXVJ (movie)

    // Extract path segments
    let path = if let Some(pos) = url.find("crunchyroll.com") {
        &url[pos + 15..] // Skip "crunchyroll.com"
    } else {
        return None;
    };

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if segments.is_empty() {
        return None;
    }

    match segments.get(0).map(|s| *s) {
        Some("series") => {
            // /series/{id}/slug
            segments.get(1).map(|id| ContentType::Series(id.to_string()))
        }
        Some("watch") => {
            // /watch/{id}/slug - could be episode or movie
            segments.get(1).map(|id| ContentType::Episode(id.to_string()))
        }
        Some(segment) if segment.starts_with("G") || segment.starts_with("g") => {
            // Direct ID in URL
            Some(ContentType::Episode(segment.to_string()))
        }
        _ => None,
    }
}

use crate::config::OwnerUser;

/// Extract owner contact usernames for display
fn get_owner_contacts(owners: &[OwnerUser]) -> Vec<String> {
    owners
        .iter()
        .filter_map(|o| o.username.as_ref().cloned())
        .collect()
}

/// Build "not authorized" message with owner contact info (full version)
pub fn build_not_authorized_message(owners: &[OwnerUser]) -> String {
    let contacts = get_owner_contacts(owners);
    let contact_info = if contacts.is_empty() {
        String::new()
    } else {
        format!("\n\n📞 Liên hệ Owner để được cấp quyền: {}", contacts.join(", "))
    };

    format!(
        "🚫 Bạn không được phép sử dụng bot này.{}",
        contact_info
    )
}

/// Build short "not authorized" message for callback alerts (max ~200 chars)
pub fn build_callback_not_authorized(owners: &[OwnerUser]) -> String {
    let contacts = get_owner_contacts(owners);
    if contacts.is_empty() {
        "🚫 Bạn không được phép sử dụng bot này.".to_string()
    } else {
        format!("🚫 Không có quyền. Liên hệ: {}", contacts.join(", "))
    }
}

/// Build welcome message with owner info
pub fn build_welcome_message(owners: &[OwnerUser]) -> String {
    let owner_info = if owners.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n👑 *Owner:* {}",
            owners
                .iter()
                .map(|o| o.display())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

//     format!(
//         r#"🎬 *Chào mừng đến với AnAlime Bot\!*

// 📋 *Các lệnh cơ bản:*
// • `/search <tên>` \- Tìm kiếm anime
// • `/get <id hoặc url>` \- Tải trực tiếp
// • `/tools` \- Kiểm tra công cụ
// • `/help` \- Xem hướng dẫn đầy đủ

// 📝 *Ví dụ:*
// `/search Attack on Titan`
// `/get GRMG8ZQZR`{}"#,
//         owner_info
//     )
    format!(
        r#"🎬 *Chào mừng đến với AnAlime Bot\!*

📋 *Các lệnh cơ bản:*
• `/search <tên>` \- Tìm kiếm anime
• `/tools` \- Kiểm tra công cụ
• `/help` \- Xem hướng dẫn đầy đủ

📝 *Ví dụ:*
`/search Attack on Titan`
{}"#,
        owner_info
    )
}

/// Build help message with owner info
pub fn build_help_message(owners: &[OwnerUser]) -> String {
    let owner_info = if owners.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n👑 *Owner:* {}",
            owners
                .iter()
                .map(|o| o.display())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

//     format!(
//         r#"📖 *Hướng Dẫn Sử Dụng AnAlime Bot*

// 🔹 *Lệnh cơ bản:*
// • `/search <tên>` \- Tìm kiếm anime theo tên
// • `/get <id hoặc url>` \- Tải bằng ID hoặc URL
// • `/cancel` \- Huỷ thao tác đang thực hiện
// • `/status` \- Xem trạng thái bot
// • `/help` \- Hiển thị hướng dẫn này

// 🔹 *Hỗ trợ ID/URL trực tiếp:*
// • `/get <series_id>` \- Xem danh sách mùa
// • `/get <episode_id>` \- Tải tập phim trực tiếp
// • `/get <movie_id>` \- Tải phim lẻ
// • `/get <url>` \- Phân tích URL và tải

// 📝 *Ví dụ:*
// • `/get GRMG8ZQZR` \(ID series\)
// • `/get GZ7UV1EPW` \(ID tập phim\)
// • `/get https://www\.crunchyroll\.com/watch/xxx`

// 🔧 *Lệnh công cụ:*
// • `/tools` \- Kiểm tra mp4decrypt và FFmpeg
// • `/installtools` \- Tự động tải công cụ còn thiếu

// 📌 *Lưu ý:*
// • Video dưới 2GB sẽ gửi trực tiếp qua Telegram
// • Video lớn hơn sẽ upload lên dịch vụ lưu trữ
// • Tự động chọn chất lượng cao nhất

// 💝 *Ủng hộ Bot:*
// • Bank Transfer: VietinBank / MBBank / OCB
//   STK: `0869261804` \- Nội dung: `donate dabeecao`
// • Momo: [me\.momo\.vn/dabeecao](https://me.momo.vn/dabeecao)
// • PayPal: [paypal\.me/dabeecao](https://paypal.me/dabeecao){}"#,
//         owner_info
//     )
    format!(
        r#"📖 *Hướng Dẫn Sử Dụng AnAlime Bot*

🔹 *Lệnh cơ bản:*
• `/search <tên>` \- Tìm kiếm anime theo tên
• `/cancel` \- Huỷ thao tác đang thực hiện
• `/status` \- Xem trạng thái bot
• `/help` \- Hiển thị hướng dẫn này

🔧 *Lệnh công cụ:*
• `/tools` \- Kiểm tra mp4decrypt và FFmpeg
• `/installtools` \- Tự động tải công cụ còn thiếu

📌 *Lưu ý:*
• Video dưới 2GB sẽ gửi trực tiếp qua Telegram
• Video lớn hơn sẽ upload lên dịch vụ lưu trữ
• Tự động chọn chất lượng cao nhất

💝 *Ủng hộ Bot:*
• Thông tin ủng hộ: [dabeecao\.org/\#donate](https://dabeecao.org/#donate){}"#,
        owner_info
    )
}

/// Build donate message
pub fn build_donate_message() -> String {
    r#"💝 *Ủng Hộ AnAlime Bot*

Cảm ơn bạn đã sử dụng bot\! Nếu bạn thấy hữu ích, hãy ủng hộ để bot tiếp tục phát triển\.

🏦 *Thông tin ủng hộ:*
[dabeecao\.org/\#donate](https://dabeecao.org/#donate)

Mọi đóng góp đều được ghi nhận và trân trọng\! 🙏"#.to_string()
}
