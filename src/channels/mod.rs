pub mod delivery;
pub mod discord;
pub mod slack;
pub mod telegram;

// Re-export adapter types
pub use discord::DiscordAdapter;
pub use slack::SlackAdapter;
pub use telegram::TelegramAdapter;
