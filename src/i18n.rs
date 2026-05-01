/// Supported UI languages
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Lang {
    #[default]
    Vi,
    En,
}

impl Lang {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "en" | "english" => Lang::En,
            _ => Lang::Vi,
        }
    }
}

/// All UI strings used by the bot
pub struct Strings {
    // Auth / access
    pub not_authorized: &'static str,
    pub not_authorized_callback: &'static str,
    pub not_callback_owner: &'static str,
    pub owner_only: &'static str,
    pub owner_or_admin_only: &'static str,

    // General errors
    pub db_not_configured: &'static str,
    pub unknown_error: &'static str,

    // Search
    pub search_empty: &'static str,
    pub searching: &'static str,   // format: "🔍 {query}..."
    pub search_not_found: &'static str, // format: "❌ '{query}'"
    pub search_found: &'static str,     // format: "🎯 N results for '{query}':"
    pub search_error: &'static str,     // format: "❌ {err}"

    // Get command
    pub get_empty: &'static str,
    pub get_loading: &'static str,    // format: "⏳ '{input}'..."
    pub get_invalid_input: &'static str,
    pub get_not_found: &'static str,   // format: "❌ '{id}'"

    // Series / Season / Episode navigation
    pub loading_seasons: &'static str,
    pub no_seasons: &'static str,
    pub seasons_select: &'static str, // format: "📺 {title}\n\n📂 select:"
    pub seasons_error: &'static str,  // format: "❌ {err}"
    pub loading_episodes: &'static str,
    pub no_episodes_season: &'static str,
    pub episodes_select: &'static str, // format: "📺 {series}\n📁 {season}\n\n🎬 N avail:"
    pub episodes_error: &'static str,
    pub loading_episode_info: &'static str,
    pub episode_info_error: &'static str,
    pub unknown_field: &'static str,
    pub no_description: &'static str,
    pub cache_available: &'static str,

    // Episode info fields
    pub field_episode: &'static str,
    pub field_duration: &'static str,
    pub field_audio: &'static str,

    // Movie
    pub no_movies: &'static str,
    pub movie_error: &'static str,
    pub movie_not_found: &'static str,

    // Download flow
    pub loading_languages: &'static str,
    pub audio_select: &'static str, // format: "🔊 N avail:"
    pub download_starting: &'static str,
    pub download_preparing: &'static str,
    pub download_progress: &'static str, // format: "⬇️ 0%"
    pub download_cancelled: &'static str,
    pub download_cancelled_subscriber: &'static str,
    pub stream_url_unavailable: &'static str,
    pub no_audio_selected: &'static str,
    pub audio_session_expired: &'static str,
    pub stream_fetch_error: &'static str, // format: "❌ {err}"
    pub download_error: &'static str,      // format: "❌ {err}"
    pub upload_error: &'static str,        // format: "❌ {err}"
    pub upload_storage_error: &'static str,
    pub forward_error: &'static str,
    pub file_too_large_no_service: &'static str,
    pub upload_fallback_prefix: &'static str,
    pub upload_switching: &'static str, // format: "🔄 {service}..."

    // Cache
    pub cache_hit_sending: &'static str,
    pub cache_miss_downloading: &'static str,
    pub cache_invalidated: &'static str,
    pub already_downloading: &'static str,

    // Cancel
    pub cancelled_ok: &'static str,

    // Tools
    pub tools_header: &'static str,
    pub tools_version: &'static str,
    pub tools_location: &'static str,
    pub tools_install_hint: &'static str,
    pub tools_installing: &'static str,
    pub tools_installed_ok: &'static str,
    pub tools_install_failed: &'static str,

    // Status
    pub status_connected: &'static str,
    pub status_disconnected: &'static str,
    pub status_message: &'static str, // format template

    // Stats
    pub stats_no_db: &'static str,
    pub stats_error: &'static str,
    pub stats_header: &'static str,          // "📈 Statistics {name}"
    pub stats_downloading: &'static str,     // "⏳ Downloading:"
    pub stats_allowed_users: &'static str,   // "👥 Allowed users:"
    pub stats_owner: &'static str,
    pub stats_admin: &'static str,
    pub stats_authorized_chats: &'static str,
    pub stats_total: &'static str,
    pub stats_episodes_decrypted: &'static str,
    pub stats_cache_telegram: &'static str,
    pub stats_cache_buzzheavier: &'static str,
    pub stats_cache_pixeldrain: &'static str,
    pub stats_cache_gofile: &'static str,
    pub stats_files: &'static str,           // "Files:"
    pub stats_served: &'static str,          // "Served: N times"
    pub stats_summary: &'static str,         // "📊 Summary:"
    pub stats_total_files: &'static str,
    pub stats_total_size: &'static str,
    pub stats_total_served: &'static str,
    pub stats_times: &'static str,           // "times" suffix for serve count

    // Admin commands
    pub add_admin_usage: &'static str,
    pub remove_admin_usage: &'static str,
    pub already_owner: &'static str,
    pub already_admin: &'static str,
    pub admin_added: &'static str,
    pub admin_add_error: &'static str,
    pub admin_not_found: &'static str,
    pub admin_removed: &'static str,
    pub admin_remove_error: &'static str,

    // Authorize
    pub authorize_usage: &'static str,
    pub deauthorize_usage: &'static str,
    pub authorize_invalid_id: &'static str,
    pub already_authorized: &'static str,
    pub authorized_ok: &'static str,
    pub authorize_error: &'static str,
    pub not_authorized_chat: &'static str,
    pub deauthorized_ok: &'static str,
    pub deauthorize_error: &'static str,

    // Back navigation
    pub back_search_hint: &'static str,
    pub back_search_hint_full: &'static str,

    // commands.rs messages
    pub not_authorized_msg: &'static str,       // "🚫 You are not allowed..."
    pub not_authorized_contact: &'static str,   // "\n\n📞 Contact owner: {owners}"
    pub not_authorized_callback_contact: &'static str, // short with contact
    pub welcome_commands: &'static str,         // the /start body (no bot name, injected by caller)
    pub help_body: &'static str,                // the /help body
    pub donate_body: &'static str,              // the /donate body
    pub owner_label: &'static str,              // "👑 Owner:"

    // Update checker
    pub update_available: &'static str, // format template

    // Bot restart notification
    pub bot_restarted: &'static str, // format template

    // System PATH fallback
    pub system_path: &'static str,

    // Keyboard labels
    pub kb_download: &'static str,
    pub kb_back: &'static str,
    pub kb_back_episodes: &'static str,
    pub kb_back_seasons: &'static str,
    pub kb_new_search: &'static str,
    pub kb_cancel_download: &'static str,
    pub kb_send_from_cache: &'static str,
    pub kb_confirm_download: &'static str, // format: "📥 N audio"

    // Episode list item
    pub ep_label: &'static str, // format: "🎬 Ep {num}: {title} ({dur})"

    // Season list item
    pub season_label: &'static str, // format: "📁 S{num}: {title} ({count} ep)"
    pub season_episodes: &'static str,

    // Progress phases
    pub phase_idle: &'static str,
    pub phase_manifest: &'static str,
    pub phase_keys: &'static str,
    pub phase_video: &'static str,
    pub phase_audio: &'static str,
    pub phase_subtitles: &'static str,
    pub phase_decrypting: &'static str,
    pub phase_muxing: &'static str,
    pub phase_uploading: &'static str,
    pub phase_completed: &'static str,
    pub phase_failed: &'static str,

    // Progress labels
    pub prog_speed: &'static str,
    pub prog_estimated: &'static str,
    pub prog_downloaded: &'static str,
    pub prog_progress: &'static str,
    pub prog_eta: &'static str,
}

static VI: Strings = Strings {
    not_authorized: "🚫 Bạn không được phép sử dụng bot này.",
    not_authorized_callback: "🚫 Không có quyền.",
    not_callback_owner: "Bạn không phải người yêu cầu!",
    owner_only: "🚫 Chỉ owner mới có thể sử dụng lệnh này.",
    owner_or_admin_only: "🚫 Chỉ owner hoặc admin mới có thể sử dụng lệnh này.",

    db_not_configured: "❌ Cơ sở dữ liệu chưa được cấu hình.",
    unknown_error: "Lỗi không xác định",

    search_empty: "⚠️ Vui lòng nhập từ khóa tìm kiếm.\n\n📝 Ví dụ: /search Attack on Titan",
    searching: "🔍 Đang tìm kiếm",
    search_not_found: "❌ Không tìm thấy kết quả cho",
    search_found: "🎯 Tìm thấy",
    search_error: "❌ Tìm kiếm thất bại",

    get_empty: "⚠️ Vui lòng cung cấp ID hoặc URL.\n\n📝 Ví dụ:\n/get GRMG8ZQZR (ID series)\n/get GZ7UV1EPW (ID tập phim)\n/get https://www.crunchyroll.com/series/GRMG8ZQZR/one-piece",
    get_loading: "⏳ Đang tra cứu",
    get_invalid_input: "⚠️ Đầu vào không hợp lệ. Vui lòng cung cấp ID hoặc URL hợp lệ.\n\n📝 Ví dụ:\n/get GRMG8ZQZR (ID series)\n/get GZ7UV1EPW (ID tập phim)\n/get https://www.crunchyroll.com/watch/GVWU0XK4Y/episode-1",
    get_not_found: "❌ Không tìm thấy nội dung",

    loading_seasons: "⏳ Đang tải danh sách các mùa...",
    no_seasons: "⚠️ Không tìm thấy mùa nào cho series này.",
    seasons_select: "📂 Vui lòng chọn một mùa phim:",
    seasons_error: "❌ Lỗi khi tải các mùa",
    loading_episodes: "⏳ Đang tải danh sách tập phim...",
    no_episodes_season: "⚠️ Không tìm thấy tập nào cho mùa này.",
    episodes_select: "🎬 Chọn một tập",
    episodes_error: "❌ Lỗi khi tải danh sách tập phim",
    loading_episode_info: "⏳ Đang tải thông tin tập phim...",
    episode_info_error: "❌ Lỗi khi tải thông tin tập phim",
    unknown_field: "Không rõ",
    no_description: "Không có mô tả",
    cache_available: "✅ <b>File có sẵn trong Cache!</b>",

    field_episode: "🔢 Tập",
    field_duration: "⏱ Thời lượng",
    field_audio: "🔊 Âm thanh",

    no_movies: "❌ Không tìm thấy phim nào.",
    movie_error: "❌ Lỗi khi tải phim",
    movie_not_found: "❌ Không tìm thấy phim",

    loading_languages: "🔊 Đang tải danh sách ngôn ngữ...",
    audio_select: "🔊 Chọn ngôn ngữ âm thanh",
    download_starting: "⬇️ Bắt đầu tải xuống...",
    download_preparing: "⬇️ Đang chuẩn bị tải xuống...",
    download_progress: "⬇️ Đang tải xuống... 0%",
    download_cancelled: "❌ Tải xuống đã huỷ. Đã dọn dẹp các tệp tạm thời.",
    download_cancelled_subscriber: "❌ Tải xuống đã bị huỷ bởi người yêu cầu ban đầu.\n\n💡 Vui lòng tự tải xuống.",
    stream_url_unavailable: "⚠️ Không có URL luồng khả dụng.",
    no_audio_selected: "⚠️ Chưa chọn ngôn ngữ nào.",
    audio_session_expired: "⚠️ Phiên chọn ngôn ngữ đã hết hạn. Vui lòng thử lại.",
    stream_fetch_error: "❌ Lỗi khi lấy thông tin luồng",
    download_error: "❌ Tải xuống thất bại",
    upload_error: "❌ Tải lên thất bại",
    upload_storage_error: "❌ Tải lên lưu trữ thất bại",
    forward_error: "❌ Lỗi khi chuyển tiếp tập tin",
    file_too_large_no_service: "⚙️ Không có dịch vụ upload nào được cấu hình.\nVui lòng cấu hình Buzzheavier, Pixeldrain hoặc Gofile trong config.toml",
    upload_fallback_prefix: "✅ Tải lên hoàn tất! (Fallback)\n\n",
    upload_switching: "🔄 Đang chuyển sang",

    cache_hit_sending: "✅ Tìm thấy trong cache! 📤 Đang gửi...",
    cache_miss_downloading: "⚠️ Không tìm thấy trong cache. ⬇️ Bắt đầu tải xuống...",
    cache_invalidated: "🔄 Phụ đề đã thay đổi. Đang tải lại...",
    already_downloading: "⏳ Tập tin này đang được tải xuống. 📤 Bạn sẽ nhận được nó khi sẵn sàng.",

    cancelled_ok: "❌ Đã huỷ thao tác.",

    tools_header: "🔧 *Trạng thái công cụ:*\n\n",
    tools_version: "📌 Phiên bản",
    tools_location: "📂 Vị trí",
    tools_install_hint: "💡 Dùng /installtools để tải công cụ còn thiếu.",
    tools_installing: "⏳ Đang kiểm tra và cài đặt công cụ...",
    tools_installed_ok: "✅ Tất cả công cụ đã được cài đặt thành công!\n\n💡 Dùng /tools để kiểm tra trạng thái.",
    tools_install_failed: "❌ Cài đặt công cụ thất bại",

    status_connected: "✅ Đã kết nối",
    status_disconnected: "❌ Ngắt kết nối",
    status_message: "📊 *Trạng thái {name}:*\n\n🔗 Kết nối: {cr}\n✅ Sẵn sàng tải xuống",

    stats_no_db: "❌ Database chưa được cấu hình. Không thể hiển thị thống kê.",
    stats_error: "❌ Lỗi khi lấy thống kê",
    stats_header: "📈 Thống Kê {name}",
    stats_downloading: "⏳ Đang tải:",
    stats_allowed_users: "👥 Người dùng được phép:",
    stats_owner: "Owner",
    stats_admin: "Admin",
    stats_authorized_chats: "Chat được cấp quyền",
    stats_total: "Tổng cộng",
    stats_episodes_decrypted: "🔐 Episodes đã giải mã:",
    stats_cache_telegram: "📦 Cache Telegram:",
    stats_cache_buzzheavier: "📦 Cache Buzzheavier:",
    stats_cache_pixeldrain: "📦 Cache Pixeldrain:",
    stats_cache_gofile: "📦 Cache Gofile:",
    stats_files: "Files",
    stats_served: "Đã phục vụ",
    stats_summary: "📊 Tổng kết:",
    stats_total_files: "Tổng files cached",
    stats_total_size: "Tổng dung lượng",
    stats_total_served: "Tổng lượt phục vụ",
    stats_times: "lần",

    add_admin_usage: "⚠️ Vui lòng reply tin nhắn của người dùng hoặc nhập ID.\n\n📝 Cú pháp: /addadmin <user_id> hoặc reply tin nhắn",
    remove_admin_usage: "⚠️ Vui lòng reply tin nhắn của người dùng hoặc nhập ID.\n\n📝 Cú pháp: /removeadmin <user_id> hoặc reply tin nhắn",
    already_owner: "ℹ️ User đã là owner rồi.",
    already_admin: "ℹ️ User đã là admin rồi.",
    admin_added: "✅ Đã thêm admin",
    admin_add_error: "❌ Lỗi khi thêm admin",
    admin_not_found: "ℹ️ User không phải admin.",
    admin_removed: "✅ Đã xoá admin",
    admin_remove_error: "❌ Lỗi khi xoá admin",

    authorize_usage: "⚠️ ID không hợp lệ.\n\n📝 Cú pháp: /authorize hoặc /authorize <chat_id>",
    deauthorize_usage: "⚠️ ID không hợp lệ.\n\n📝 Cú pháp: /deauthorize hoặc /deauthorize <chat_id>",
    authorize_invalid_id: "⚠️ ID không hợp lệ.",
    already_authorized: "ℹ️ Chat đã được cấp quyền rồi.",
    authorized_ok: "✅ Đã cấp quyền cho chat",
    authorize_error: "❌ Lỗi khi cấp quyền",
    not_authorized_chat: "ℹ️ Chat chưa được cấp quyền.",
    deauthorized_ok: "✅ Đã thu hồi quyền cho chat",
    deauthorize_error: "❌ Lỗi khi thu hồi quyền",

    back_search_hint: "🔍 Sử dụng /search <tên>",
    back_search_hint_full: "🔍 Sử dụng /search để bắt đầu tìm kiếm mới.",

    not_authorized_msg: "🚫 Bạn không được phép sử dụng bot này.",
    not_authorized_contact: "\n\n📞 Liên hệ Owner để được cấp quyền: {owners}",
    not_authorized_callback_contact: "🚫 Không có quyền. Liên hệ: {owners}",
    welcome_commands: "📋 *Các lệnh cơ bản:*\n• `/search <tên>` \\- Tìm kiếm anime\n• `/tools` \\- Kiểm tra công cụ\n• `/help` \\- Xem hướng dẫn đầy đủ\n\n📝 *Ví dụ:*\n`/search Attack on Titan`",
    help_body: "🔹 *Lệnh cơ bản:*\n• `/search <tên>` \\- Tìm kiếm anime theo tên\n• `/cancel` \\- Huỷ thao tác đang thực hiện\n• `/status` \\- Xem trạng thái bot\n• `/help` \\- Hiển thị hướng dẫn này\n\n🔧 *Lệnh công cụ:*\n• `/tools` \\- Kiểm tra mp4decrypt và FFmpeg\n• `/installtools` \\- Tự động tải công cụ còn thiếu\n\n📌 *Lưu ý:*\n• Video dưới 2GB sẽ gửi trực tiếp qua Telegram\n• Video lớn hơn sẽ upload lên dịch vụ lưu trữ\n• Tự động chọn chất lượng cao nhất\n\n💝 *Ủng hộ Bot:*\n• Thông tin ủng hộ: [dabeecao\\.org/\\#donate](https://dabeecao.org/#donate)",
    donate_body: "Cảm ơn bạn đã sử dụng bot\\! Nếu bạn thấy hữu ích, hãy ủng hộ để bot tiếp tục phát triển\\.\n\n🏦 *Thông tin ủng hộ:*\n[dabeecao\\.org/\\#donate](https://dabeecao.org/#donate)\n\nMọi đóng góp đều được ghi nhận và trân trọng\\! 🙏",
    owner_label: "👑 *Owner:*",

    update_available: "🆕 *Có phiên bản mới\\!*\n\nPhiên bản hiện tại: `v{cur}`\nPhiên bản mới: `{tag}`\n\n[Tải xuống tại đây]({url})",
    bot_restarted: "⚠️ Bot đã được khởi động lại!\n\n🎬 Tải xuống của bạn đã bị gián đoạn:\n📺 {title}\n📊 Trạng thái: {phase} ({pct}%)\n\n🔄 Nếu bạn vẫn cần tập này, vui lòng yêu cầu lại.",

    system_path: "System PATH",

    kb_download: "📥 Tải xuống",
    kb_back: "⬅️ Quay lại",
    kb_back_episodes: "⬅️ Quay lại danh sách tập",
    kb_back_seasons: "⬅️ Quay lại danh sách mùa",
    kb_new_search: "🔍 Tìm kiếm mới",
    kb_cancel_download: "❌ Huỷ tải xuống",
    kb_send_from_cache: "🚀 Gửi ngay từ Cache",
    kb_confirm_download: "📥 Tải xuống",

    ep_label: "🎬 Tập",
    season_label: "📁 S",
    season_episodes: "tập",

    phase_idle: "⏳ Đang chờ...",
    phase_manifest: "📋 Đang lấy manifest...",
    phase_keys: "🔑 Đang lấy khóa giải mã...",
    phase_video: "🎬 Đang tải Video",
    phase_audio: "🔊 Đang tải Audio",
    phase_subtitles: "📝 Đang tải phụ đề...",
    phase_decrypting: "🔓 Đang giải mã nội dung...",
    phase_muxing: "🎞️ Đang ghép video, audio & phụ đề...",
    phase_uploading: "📤 Đang tải lên Telegram",
    phase_completed: "✅ Tải xuống hoàn tất!",
    phase_failed: "❌ Thất bại",

    prog_speed: "⚡ Tốc độ",
    prog_estimated: "📦 Ước tính",
    prog_downloaded: "⬇️ Đã tải",
    prog_progress: "📊 Tiến độ",
    prog_eta: "⏳ ETA",
};

static EN: Strings = Strings {
    not_authorized: "🚫 You are not authorized to use this bot.",
    not_authorized_callback: "🚫 Not authorized.",
    not_callback_owner: "You are not the requester!",
    owner_only: "🚫 Only owners can use this command.",
    owner_or_admin_only: "🚫 Only owners or admins can use this command.",

    db_not_configured: "❌ Database is not configured.",
    unknown_error: "Unknown error",

    search_empty: "⚠️ Please enter a search keyword.\n\n📝 Example: /search Attack on Titan",
    searching: "🔍 Searching",
    search_not_found: "❌ No results found for",
    search_found: "🎯 Found",
    search_error: "❌ Search failed",

    get_empty: "⚠️ Please provide an ID or URL.\n\n📝 Examples:\n/get GRMG8ZQZR (series ID)\n/get GZ7UV1EPW (episode ID)\n/get https://www.crunchyroll.com/series/GRMG8ZQZR/one-piece",
    get_loading: "⏳ Looking up",
    get_invalid_input: "⚠️ Invalid input. Please provide a valid ID or URL.\n\n📝 Examples:\n/get GRMG8ZQZR (series ID)\n/get GZ7UV1EPW (episode ID)\n/get https://www.crunchyroll.com/watch/GVWU0XK4Y/episode-1",
    get_not_found: "❌ Content not found",

    loading_seasons: "⏳ Loading seasons...",
    no_seasons: "⚠️ No seasons found for this series.",
    seasons_select: "📂 Please select a season:",
    seasons_error: "❌ Error loading seasons",
    loading_episodes: "⏳ Loading episodes...",
    no_episodes_season: "⚠️ No episodes found for this season.",
    episodes_select: "🎬 Select an episode",
    episodes_error: "❌ Error loading episodes",
    loading_episode_info: "⏳ Loading episode info...",
    episode_info_error: "❌ Error loading episode info",
    unknown_field: "Unknown",
    no_description: "No description",
    cache_available: "✅ <b>File available in Cache!</b>",

    field_episode: "🔢 Episode",
    field_duration: "⏱ Duration",
    field_audio: "🔊 Audio",

    no_movies: "❌ No movies found.",
    movie_error: "❌ Error loading movie",
    movie_not_found: "❌ Movie not found",

    loading_languages: "🔊 Loading language list...",
    audio_select: "🔊 Select audio language",
    download_starting: "⬇️ Starting download...",
    download_preparing: "⬇️ Preparing download...",
    download_progress: "⬇️ Downloading... 0%",
    download_cancelled: "❌ Download cancelled. Temp files cleaned up.",
    download_cancelled_subscriber: "❌ Download was cancelled by the original requester.\n\n💡 Please download it yourself.",
    stream_url_unavailable: "⚠️ No stream URL available.",
    no_audio_selected: "⚠️ No language selected.",
    audio_session_expired: "⚠️ Language selection session expired. Please try again.",
    stream_fetch_error: "❌ Error fetching stream info",
    download_error: "❌ Download failed",
    upload_error: "❌ Upload failed",
    upload_storage_error: "❌ Storage upload failed",
    forward_error: "❌ Error forwarding file",
    file_too_large_no_service: "⚙️ No upload service configured.\nPlease configure Buzzheavier, Pixeldrain or Gofile in config.toml",
    upload_fallback_prefix: "✅ Upload complete! (Fallback)\n\n",
    upload_switching: "🔄 Switching to",

    cache_hit_sending: "✅ Found in cache! 📤 Sending...",
    cache_miss_downloading: "⚠️ Not found in cache. ⬇️ Starting download...",
    cache_invalidated: "🔄 Subtitles changed. Re-downloading...",
    already_downloading: "⏳ This episode is already being downloaded. 📤 You'll receive it when ready.",

    cancelled_ok: "❌ Operation cancelled.",

    tools_header: "🔧 *Tool status:*\n\n",
    tools_version: "📌 Version",
    tools_location: "📂 Location",
    tools_install_hint: "💡 Use /installtools to download missing tools.",
    tools_installing: "⏳ Checking and installing tools...",
    tools_installed_ok: "✅ All tools installed successfully!\n\n💡 Use /tools to check status.",
    tools_install_failed: "❌ Tool installation failed",

    status_connected: "✅ Connected",
    status_disconnected: "❌ Disconnected",
    status_message: "📊 *{name} Status:*\n\n🔗 Connection: {cr}\n✅ Ready to download",

    stats_no_db: "❌ Database not configured. Cannot show statistics.",
    stats_error: "❌ Error fetching statistics",
    stats_header: "📈 Statistics {name}",
    stats_downloading: "⏳ Downloading:",
    stats_allowed_users: "👥 Allowed users:",
    stats_owner: "Owner",
    stats_admin: "Admin",
    stats_authorized_chats: "Authorized chats",
    stats_total: "Total",
    stats_episodes_decrypted: "🔐 Episodes decrypted:",
    stats_cache_telegram: "📦 Telegram cache:",
    stats_cache_buzzheavier: "📦 Buzzheavier cache:",
    stats_cache_pixeldrain: "📦 Pixeldrain cache:",
    stats_cache_gofile: "📦 Gofile cache:",
    stats_files: "Files",
    stats_served: "Served",
    stats_summary: "📊 Summary:",
    stats_total_files: "Total cached files",
    stats_total_size: "Total size",
    stats_total_served: "Total served",
    stats_times: "times",

    add_admin_usage: "⚠️ Please reply to a user's message or enter their ID.\n\n📝 Syntax: /addadmin <user_id> or reply to message",
    remove_admin_usage: "⚠️ Please reply to a user's message or enter their ID.\n\n📝 Syntax: /removeadmin <user_id> or reply to message",
    already_owner: "ℹ️ User is already an owner.",
    already_admin: "ℹ️ User is already an admin.",
    admin_added: "✅ Admin added",
    admin_add_error: "❌ Error adding admin",
    admin_not_found: "ℹ️ User is not an admin.",
    admin_removed: "✅ Admin removed",
    admin_remove_error: "❌ Error removing admin",

    authorize_usage: "⚠️ Invalid ID.\n\n📝 Syntax: /authorize or /authorize <chat_id>",
    deauthorize_usage: "⚠️ Invalid ID.\n\n📝 Syntax: /deauthorize or /deauthorize <chat_id>",
    authorize_invalid_id: "⚠️ Invalid ID.",
    already_authorized: "ℹ️ Chat is already authorized.",
    authorized_ok: "✅ Chat authorized",
    authorize_error: "❌ Error authorizing chat",
    not_authorized_chat: "ℹ️ Chat is not authorized.",
    deauthorized_ok: "✅ Authorization revoked for chat",
    deauthorize_error: "❌ Error revoking authorization",

    back_search_hint: "🔍 Use /search <name>",
    back_search_hint_full: "🔍 Use /search to start a new search.",

    not_authorized_msg: "🚫 You are not allowed to use this bot.",
    not_authorized_contact: "\n\n📞 Contact the owner to get access: {owners}",
    not_authorized_callback_contact: "🚫 No permission. Contact: {owners}",
    welcome_commands: "📋 *Basic commands:*\n• `/search <name>` \\- Search for anime\n• `/tools` \\- Check tools\n• `/help` \\- View full guide\n\n📝 *Example:*\n`/search Attack on Titan`",
    help_body: "🔹 *Basic commands:*\n• `/search <name>` \\- Search for anime\n• `/cancel` \\- Cancel current operation\n• `/status` \\- View bot status\n• `/help` \\- Show this guide\n\n🔧 *Tool commands:*\n• `/tools` \\- Check mp4decrypt and FFmpeg\n• `/installtools` \\- Auto\\-download missing tools\n\n📌 *Notes:*\n• Videos under 2GB sent directly via Telegram\n• Larger files uploaded to external service\n• Automatically selects highest quality\n\n💝 *Support:*\n• [dabeecao\\.org/\\#donate](https://dabeecao.org/#donate)",
    donate_body: "Thank you for using this bot\\! If you find it useful, please support its development\\.\n\n🏦 *Donation info:*\n[dabeecao\\.org/\\#donate](https://dabeecao.org/#donate)\n\nAll contributions are appreciated\\! 🙏",
    owner_label: "👑 *Owner:*",

    update_available: "🆕 *New version available\\!*\n\nCurrent version: `v{cur}`\nNew version: `{tag}`\n\n[Download here]({url})",
    bot_restarted: "⚠️ Bot has been restarted!\n\n🎬 Your download was interrupted:\n📺 {title}\n📊 Status: {phase} ({pct}%)\n\n🔄 If you still need this episode, please request it again.",

    system_path: "System PATH",

    kb_download: "📥 Download",
    kb_back: "⬅️ Back",
    kb_back_episodes: "⬅️ Back to episode list",
    kb_back_seasons: "⬅️ Back to season list",
    kb_new_search: "🔍 New search",
    kb_cancel_download: "❌ Cancel download",
    kb_send_from_cache: "🚀 Send from Cache",
    kb_confirm_download: "📥 Download",

    ep_label: "🎬 Ep",
    season_label: "📁 S",
    season_episodes: "ep",

    phase_idle: "⏳ Waiting...",
    phase_manifest: "📋 Fetching manifest...",
    phase_keys: "🔑 Fetching decryption keys...",
    phase_video: "🎬 Downloading Video",
    phase_audio: "🔊 Downloading Audio",
    phase_subtitles: "📝 Downloading subtitles...",
    phase_decrypting: "🔓 Decrypting content...",
    phase_muxing: "🎞️ Muxing video, audio & subtitles...",
    phase_uploading: "📤 Uploading to Telegram",
    phase_completed: "✅ Download complete!",
    phase_failed: "❌ Failed",

    prog_speed: "⚡ Speed",
    prog_estimated: "📦 Estimated",
    prog_downloaded: "⬇️ Downloaded",
    prog_progress: "📊 Progress",
    prog_eta: "⏳ ETA",
};

impl Strings {
    pub fn get(lang: &Lang) -> &'static Strings {
        match lang {
            Lang::Vi => &VI,
            Lang::En => &EN,
        }
    }
}
