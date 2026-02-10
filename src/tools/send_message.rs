use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use teloxide::prelude::*;

use super::{authorize_chat_access, schema_object, Tool, ToolResult};
use crate::channel::{deliver_and_store_bot_message, enforce_channel_policy};
use crate::claude::ToolDefinition;
use crate::db::Database;

pub struct SendMessageTool {
    bot: Bot,
    db: Arc<Database>,
    bot_username: String,
}

impl SendMessageTool {
    pub fn new(bot: Bot, db: Arc<Database>, bot_username: String) -> Self {
        SendMessageTool {
            bot,
            db,
            bot_username,
        }
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "send_message".into(),
            description: "Send a message mid-conversation. For Telegram chats it sends via Telegram; for local web chats it appends to the web conversation.".into(),
            input_schema: schema_object(
                json!({
                    "chat_id": {
                        "type": "integer",
                        "description": "The target chat ID"
                    },
                    "text": {
                        "type": "string",
                        "description": "The message text to send"
                    }
                }),
                &["chat_id", "text"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let chat_id = match input.get("chat_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return ToolResult::error("Missing required parameter: chat_id".into()),
        };
        let text = match input.get("text").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return ToolResult::error("Missing required parameter: text".into()),
        };

        if let Err(e) = authorize_chat_access(&input, chat_id) {
            return ToolResult::error(e);
        }

        if let Err(e) = enforce_channel_policy(self.db.clone(), &input, chat_id).await {
            return ToolResult::error(e);
        }

        match deliver_and_store_bot_message(
            &self.bot,
            self.db.clone(),
            &self.bot_username,
            chat_id,
            text,
        )
        .await
        {
            Ok(_) => ToolResult::success("Message sent successfully.".into()),
            Err(e) => ToolResult::error(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_db() -> (Arc<Database>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("microclaw_sendmsg_{}", uuid::Uuid::new_v4()));
        let db = Arc::new(Database::new(dir.to_str().unwrap()).unwrap());
        (db, dir)
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_send_message_permission_denied_before_network() {
        let (db, dir) = test_db();
        let tool = SendMessageTool::new(Bot::new("123456:TEST_TOKEN"), db, "bot".into());
        let result = tool
            .execute(json!({
                "chat_id": 200,
                "text": "hello",
                "__microclaw_auth": {
                    "caller_chat_id": 100,
                    "control_chat_ids": []
                }
            }))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("Permission denied"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_send_message_web_target_writes_to_db() {
        let (db, dir) = test_db();
        db.upsert_chat(999, Some("web-main"), "web").unwrap();

        let tool = SendMessageTool::new(Bot::new("123456:TEST_TOKEN"), db.clone(), "bot".into());
        let result = tool
            .execute(json!({
                "chat_id": 999,
                "text": "hello web",
                "__microclaw_auth": {
                    "caller_chat_id": 999,
                    "control_chat_ids": []
                }
            }))
            .await;
        assert!(!result.is_error, "{}", result.content);

        let all = db.get_all_messages(999).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].content, "hello web");
        assert!(all[0].is_from_bot);
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_send_message_web_caller_cross_chat_denied() {
        let (db, dir) = test_db();
        db.upsert_chat(100, Some("web-main"), "web").unwrap();
        db.upsert_chat(200, Some("tg"), "private").unwrap();

        let tool = SendMessageTool::new(Bot::new("123456:TEST_TOKEN"), db, "bot".into());
        let result = tool
            .execute(json!({
                "chat_id": 200,
                "text": "hello",
                "__microclaw_auth": {
                    "caller_chat_id": 100,
                    "control_chat_ids": [100]
                }
            }))
            .await;
        assert!(result.is_error);
        assert!(result
            .content
            .contains("web chats cannot operate on other chats"));
        cleanup(&dir);
    }
}
