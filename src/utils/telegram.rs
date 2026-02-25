//! Telegram-specific utility functions

use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardMarkup, MessageId, ReplyParameters};

/// Helper to send completion message
/// For group chats: delete old message and send a new one (so it appears at bottom)
/// For private chats: just edit the existing message
pub async fn send_completion_message(
    bot: &Bot,
    chat_id: ChatId,
    old_msg_id: MessageId,
    reply_to_msg_id: Option<MessageId>,
    message: String,
    keyboard: InlineKeyboardMarkup,
) {
    // Check if this is a group chat (negative chat_id indicates group/supergroup)
    let is_group = chat_id.0 < 0;

    if is_group {
        // For group chats: delete old message and send new one at bottom
        let _ = bot.delete_message(chat_id, old_msg_id).await;

        // Send new message, optionally replying to the original request
        let mut send_msg = bot.send_message(chat_id, &message).reply_markup(keyboard);
        if let Some(reply_id) = reply_to_msg_id {
            send_msg = send_msg.reply_parameters(ReplyParameters::new(reply_id));
        }
        let _ = send_msg.await;
    } else {
        // For private chats: just edit the existing message
        let _ = bot.edit_message_text(chat_id, old_msg_id, &message)
            .reply_markup(keyboard)
            .await;
    }
}

/// Extract and validate user_id from callback data
/// Returns None if the callback is not for this user
pub fn validate_callback_user(parts: &[&str], callback_user_id: i64) -> Option<i64> {
    // The last part should be the user_id
    if let Some(last) = parts.last() {
        if let Ok(owner_id) = last.parse::<i64>() {
            if owner_id == callback_user_id {
                return Some(owner_id);
            }
        }
    }
    None
}
