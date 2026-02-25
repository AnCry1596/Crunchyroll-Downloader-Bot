use crate::crunchyroll::types::{Episode, SearchItem, Season, Version};
use crate::drm::decrypt::Muxer;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

/// Maximum button text length
const MAX_BUTTON_TEXT: usize = 40;

/// Truncate string to max length with ellipsis
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

/// Build keyboard for search results (locked to user)
pub fn search_results_keyboard(items: &[SearchItem], user_id: i64) -> InlineKeyboardMarkup {
    let buttons: Vec<Vec<InlineKeyboardButton>> = items
        .iter()
        .take(10) // Limit to 10 results
        .map(|item| {
            let label = truncate(&item.title, MAX_BUTTON_TEXT);
            vec![InlineKeyboardButton::callback(
                label,
                format!("series:{}:{}", item.id, user_id),
            )]
        })
        .collect();

    InlineKeyboardMarkup::new(buttons)
}

/// Build keyboard for seasons (locked to user)
pub fn seasons_keyboard(seasons: &[Season], series_id: &str, user_id: i64) -> InlineKeyboardMarkup {
    let mut buttons: Vec<Vec<InlineKeyboardButton>> = seasons
        .iter()
        .map(|s| {
            let season_num = s.season_sequence_number.or(s.season_number).unwrap_or(0);
            let ep_count = s.number_of_episodes.unwrap_or(0);
            let label = format!(
                "📁 S{}: {} ({} tập)",
                season_num,
                truncate(&s.title, 22),
                ep_count
            );
            vec![InlineKeyboardButton::callback(
                label,
                format!("season:{}:{}:{}", series_id, s.id, user_id),
            )]
        })
        .collect();

    // Add back button
    buttons.push(vec![InlineKeyboardButton::callback(
        "⬅️ Quay lại",
        format!("back:search:{}", user_id),
    )]);

    InlineKeyboardMarkup::new(buttons)
}

/// Build keyboard for episodes (paginated, locked to user)
pub fn episodes_keyboard(
    episodes: &[Episode],
    season_id: &str,
    series_id: &str,
    page: usize,
    page_size: usize,
    user_id: i64,
) -> InlineKeyboardMarkup {
    let start = page * page_size;
    let end = std::cmp::min(start + page_size, episodes.len());
    let total_pages = (episodes.len() + page_size - 1) / page_size;

    let mut buttons: Vec<Vec<InlineKeyboardButton>> = episodes[start..end]
        .iter()
        .map(|ep| {
            let label = format!(
                "🎬 Tập {}: {} ({})",
                ep.display_number(),
                truncate(&ep.title, 20),
                ep.duration_formatted()
            );
            vec![InlineKeyboardButton::callback(
                label,
                format!("episode:{}:{}", ep.id, user_id),
            )]
        })
        .collect();

    // Pagination buttons
    if total_pages > 1 {
        let mut nav_buttons = Vec::new();

        if page > 0 {
            nav_buttons.push(InlineKeyboardButton::callback(
                "⬅️ Trước",
                format!("page:{}:{}:{}:{}", season_id, series_id, page - 1, user_id),
            ));
        }

        nav_buttons.push(InlineKeyboardButton::callback(
            format!("📄 {}/{}", page + 1, total_pages),
            "noop".to_string(),
        ));

        if page < total_pages - 1 {
            nav_buttons.push(InlineKeyboardButton::callback(
                "Sau ➡️",
                format!("page:{}:{}:{}:{}", season_id, series_id, page + 1, user_id),
            ));
        }

        buttons.push(nav_buttons);
    }

    // Back button
    buttons.push(vec![InlineKeyboardButton::callback(
        "⬅️ Quay lại danh sách mùa",
        format!("series:{}:{}", series_id, user_id),
    )]);

    InlineKeyboardMarkup::new(buttons)
}

/// Build keyboard for episode actions (locked to user)
pub fn episode_actions_keyboard(episode_id: &str, season_id: &str, user_id: i64) -> InlineKeyboardMarkup {
    episode_actions_keyboard_with_pixeldrain(episode_id, season_id, false, user_id)
}

/// Build keyboard for episode actions with optional Pixeldrain (locked to user)
pub fn episode_actions_keyboard_with_pixeldrain(
    episode_id: &str,
    season_id: &str,
    pixeldrain_enabled: bool,
    user_id: i64,
) -> InlineKeyboardMarkup {
    let mut buttons = vec![];

    // Only show Pixeldrain if enabled, otherwise show Telegram
    if pixeldrain_enabled {
        buttons.push(vec![InlineKeyboardButton::callback(
            "📥 Tải xuống",
            format!("pixeldrain:{}:{}", episode_id, user_id),
        )]);
    } else {
        buttons.push(vec![InlineKeyboardButton::callback(
            "📥 Tải xuống",
            format!("download:{}:{}", episode_id, user_id),
        )]);
    }

    // Only show back button if we have a valid season_id
    if !season_id.is_empty() {
        buttons.push(vec![InlineKeyboardButton::callback(
            "⬅️ Quay lại danh sách tập",
            format!("back:season:{}:{}", season_id, user_id),
        )]);
    } else {
        // No season context - offer new search
        buttons.push(vec![InlineKeyboardButton::callback(
            "🔍 Tìm kiếm mới",
            format!("back:search:{}", user_id),
        )]);
    }

    InlineKeyboardMarkup::new(buttons)
}

/// Build keyboard for episode actions with series context (locked to user)
pub fn episode_actions_keyboard_full(
    episode_id: &str,
    season_id: &str,
    series_id: &str,
    user_id: i64,
) -> InlineKeyboardMarkup {
    let mut buttons = vec![vec![InlineKeyboardButton::callback(
        "📥 Tải chất lượng cao nhất",
        format!("download:{}:{}", episode_id, user_id),
    )]];

    // Back button depends on what context we have
    if !season_id.is_empty() && !series_id.is_empty() {
        buttons.push(vec![InlineKeyboardButton::callback(
            "⬅️ Quay lại danh sách tập",
            format!("season:{}:{}:{}", series_id, season_id, user_id),
        )]);
    } else if !series_id.is_empty() {
        buttons.push(vec![InlineKeyboardButton::callback(
            "⬅️ Quay lại danh sách mùa",
            format!("series:{}:{}", series_id, user_id),
        )]);
    } else {
        buttons.push(vec![InlineKeyboardButton::callback(
            "🔍 Tìm kiếm mới",
            format!("back:search:{}", user_id),
        )]);
    }

    InlineKeyboardMarkup::new(buttons)
}

/// Build keyboard for download progress (locked to user)
pub fn download_progress_keyboard(task_id: &str, episode_id: &str, user_id: i64) -> InlineKeyboardMarkup {
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "❌ Huỷ tải xuống",
        format!("cancel:{}:{}:{}", task_id, episode_id, user_id),
    )]];

    InlineKeyboardMarkup::new(buttons)
}

/// Build keyboard for download complete with donate buttons
pub fn download_complete_keyboard(_episode_id: &str, _user_id: i64) -> InlineKeyboardMarkup {
    let buttons = vec![
        vec![
            InlineKeyboardButton::url("💝 Donate", "https://dabeecao.org/#donate".parse().unwrap()),
        ],
    ];

    InlineKeyboardMarkup::new(buttons)
}

/// Build audio language selection keyboard with toggle checkboxes
pub fn audio_selection_keyboard(
    versions: &[Version],
    selected_indices: &[usize],
    episode_id: &str,
    user_id: i64,
) -> InlineKeyboardMarkup {
    let mut buttons: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    for (idx, version) in versions.iter().enumerate() {
        let locale = version.audio_locale.as_deref().unwrap_or("unknown");
        let display_name = Muxer::locale_to_name(locale);
        let is_selected = selected_indices.contains(&idx);
        let prefix = if is_selected { "✅" } else { "⬜" };
        let original_tag = if version.original == Some(true) { " (Original)" } else { "" };

        let label = format!("{} {}{}", prefix, display_name, original_tag);
        let callback = format!("as:{}:{}:{}", episode_id, idx, user_id);

        buttons.push(vec![InlineKeyboardButton::callback(label, callback)]);
    }

    // Confirm button with count
    let selected_count = selected_indices.len();
    let confirm_label = format!("📥 Tải xuống ({} audio)", selected_count);
    let confirm_callback = format!("ad:{}:{}", episode_id, user_id);
    buttons.push(vec![InlineKeyboardButton::callback(confirm_label, confirm_callback)]);

    // Back button
    buttons.push(vec![InlineKeyboardButton::callback(
        "⬅️ Quay lại",
        format!("back:search:{}", user_id),
    )]);

    InlineKeyboardMarkup::new(buttons)
}

/// Build confirmation keyboard (locked to user)
pub fn confirm_keyboard(action: &str, cancel_action: &str, user_id: i64) -> InlineKeyboardMarkup {
    let buttons = vec![vec![
        InlineKeyboardButton::callback("✅ Có", format!("{}:{}", action, user_id)),
        InlineKeyboardButton::callback("❌ Không", format!("{}:{}", cancel_action, user_id)),
    ]];

    InlineKeyboardMarkup::new(buttons)
}
