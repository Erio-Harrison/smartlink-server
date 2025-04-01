use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MessageType {
    UserStatus,
    NewMessage,
    MessageStatusUpdate,
    WebRTCSignal,
    TypingIndicator,
    ConversationUpdated,
    GroupMemberAdded,
    GroupMemberRemoved,
    SystemNotification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebSocketMessage {
    message_type: MessageType,
    sender_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recipient_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    timestamp: String,
}

// 消息发送者映射，用于追踪消息ID对应的发送者
type MessageSenderMap = HashMap<String, String>;

// 在线用户映射，用于追踪用户ID对应的消息发送通道
type UserConnectionMap = HashMap<String, mpsc::UnboundedSender<Message>>;

// 应用状态
struct AppState {
    user_connections: RwLock<UserConnectionMap>,
    message_senders: RwLock<MessageSenderMap>,
}

impl AppState {
    fn new() -> Self {
        Self {
            user_connections: RwLock::new(UserConnectionMap::new()),
            message_senders: RwLock::new(MessageSenderMap::new()),
        }
    }

    // 添加用户连接
    async fn add_user_connection(&self, user_id: String, sender: mpsc::UnboundedSender<Message>) {
        let mut connections = self.user_connections.write().await;
        
        // 检查是否已存在连接，如果存在可能需要关闭旧连接
        if connections.contains_key(&user_id) {
            info!("User {} already connected, replacing connection", user_id);
        }
        
        connections.insert(user_id, sender);
    }

    // 移除用户连接
    async fn remove_user_connection(&self, user_id: &str) {
        let mut connections = self.user_connections.write().await;
        if connections.remove(user_id).is_some() {
            info!("Removed connection for user {}", user_id);
        }
    }

    // 发送消息给指定用户
    async fn send_to_user(&self, user_id: &str, message: &WebSocketMessage) -> bool {
        let connections = self.user_connections.read().await;
        
        if let Some(sender) = connections.get(user_id) {
            match serde_json::to_string(message) {
                Ok(message_str) => {
                    if let Err(e) = sender.send(Message::Text(message_str)) {
                        error!("Failed to send message to user {}: {}", user_id, e);
                        return false;
                    }
                    return true;
                }
                Err(e) => {
                    error!("Failed to serialize message: {}", e);
                    return false;
                }
            }
        }
        
        // 用户不在线
        debug!("User {} is not online", user_id);
        false
    }

    // 记录消息发送者
    async fn record_message_sender(&self, message_id: String, sender_id: String) {
        let mut senders = self.message_senders.write().await;
        senders.insert(message_id, sender_id);
    }

    // 获取消息发送者
    async fn get_message_sender(&self, message_id: &str) -> Option<String> {
        let senders = self.message_senders.read().await;
        senders.get(message_id).cloned()
    }

    // 获取在线用户列表
    async fn get_online_users(&self) -> Vec<String> {
        let connections = self.user_connections.read().await;
        connections.keys().cloned().collect()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    
    // 创建应用状态
    let state = Arc::new(AppState::new());
    
    // 监听WebSocket连接
    let addr = "0.0.0.0:8080";
    let listener = TcpListener::bind(addr).await?;
    info!("WebSocket server listening on ws://{}", addr);
    
    // 处理连接
    while let Ok((stream, addr)) = listener.accept().await {
        info!("Received connection from: {}", addr);
        
        // 克隆应用状态以供任务使用
        let state = state.clone();
        
        // 为每个连接创建一个任务
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, state).await {
                error!("Error handling connection: {}", e);
            }
        });
    }
    
    Ok(())
}

// 处理WebSocket连接
async fn handle_connection(
    stream: TcpStream,
    state: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 接受WebSocket连接
    let ws_stream = accept_async(stream).await?;
    info!("WebSocket connection established");
    
    // 创建发送通道
    let (sender, receiver) = mpsc::unbounded_channel();
    
    // 分离WebSocket读写流
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    
    // 创建用户ID变量，初始为None
    let mut user_id = None;
    
    // 处理发送任务 - 从通道接收消息并发送到WebSocket
    let mut send_task = tokio::spawn(async move {
        let mut receiver = receiver;
        while let Some(message) = receiver.recv().await {
            if ws_sender.send(message).await.is_err() {
                break;
            }
        }
    });
    
    // 处理第一条消息以识别用户
    if let Some(result) = ws_receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                // 解析认证消息
                match serde_json::from_str::<WebSocketMessage>(&text) {
                    Ok(message) => {
                        if let MessageType::UserStatus = message.message_type {
                            // 提取用户ID
                            let uid = message.sender_id.clone();
                            info!("User authenticated: {}", uid);
                            
                            // 保存连接
                            state.add_user_connection(uid.clone(), sender.clone()).await;
                            user_id = Some(uid);
                        } else {
                            error!("First message is not a user status message");
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse authentication message: {}", e);
                        return Ok(());
                    }
                }
            }
            _ => {
                error!("Expected text message for authentication");
                return Ok(());
            }
        }
    } else {
        error!("Connection closed before authentication");
        return Ok(());
    }
    
    // 主消息处理循环
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                if let Some(ref uid) = user_id {
                    process_message(text, uid, &state).await;
                }
            }
            Ok(Message::Ping(data)) => {
                // 响应Ping消息
                if sender.send(Message::Pong(data)).is_err() {
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                // 连接关闭
                info!("Connection closed by client");
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {} // 忽略其他类型的消息
        }
    }
    
    // 清理连接
    if let Some(uid) = user_id {
        state.remove_user_connection(&uid).await;
    }
    
    // 终止发送任务
    send_task.abort();
    
    Ok(())
}

// 处理接收到的消息
async fn process_message(text: String, sender_id: &str, state: &Arc<AppState>) {
    // 解析消息
    let message = match serde_json::from_str::<WebSocketMessage>(&text) {
        Ok(msg) => msg,
        Err(e) => {
            error!("Failed to parse message: {}", e);
            return;
        }
    };
    
    // 处理不同类型的消息
    match message.message_type {
        MessageType::NewMessage => {
            handle_new_message(message, state).await;
        }
        MessageType::MessageStatusUpdate => {
            handle_message_status_update(message, state).await;
        }
        MessageType::WebRTCSignal => {
            handle_webrtc_signal(message, state).await;
        }
        MessageType::TypingIndicator => {
            handle_typing_indicator(message, state).await;
        }
        MessageType::ConversationUpdated => {
            handle_conversation_updated(message, state).await;
        }
        MessageType::GroupMemberAdded | MessageType::GroupMemberRemoved => {
            handle_group_member_update(message, state).await;
        }
        MessageType::UserStatus => {
            // 已在连接处理中处理，这里忽略
        }
        MessageType::SystemNotification => {
            // 系统通知，转发给指定用户
            if let Some(recipient_id) = &message.recipient_id {
                state.send_to_user(recipient_id, &message).await;
            }
        }
    }
}

// 处理新消息通知
async fn handle_new_message(message: WebSocketMessage, state: &Arc<AppState>) {
    // 如果有消息ID，记录发送者
    if let Some(message_id) = &message.message_id {
        state.record_message_sender(message_id.clone(), message.sender_id.clone()).await;
    }
    
    // 从data字段获取接收者列表
    let recipients = if let Some(data) = &message.data {
        if let Some(recipients) = data.get("recipients").and_then(|r| r.as_array()) {
            recipients.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<String>>()
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    
    // 如果有conversation_id，可能是群聊，需要通知所有参与者
    if let Some(conversation_id) = &message.conversation_id {
        debug!("Processing new message for conversation {}", conversation_id);
        
        // 转发消息给接收者
        for recipient_id in &recipients {
            if recipient_id != &message.sender_id {
                state.send_to_user(recipient_id, &message).await;
            }
        }
    } else if let Some(recipient_id) = &message.recipient_id {
        // 私聊消息
        debug!("Processing new message for recipient {}", recipient_id);
        state.send_to_user(recipient_id, &message).await;
    }
}

// 处理消息状态更新
async fn handle_message_status_update(message: WebSocketMessage, state: &Arc<AppState>) {
    if let Some(message_id) = &message.message_id {
        if let Some(data) = &message.data {
            // 获取原始发送者
            if let Some(original_sender_id) = data.get("originalSenderId").and_then(|s| s.as_str()) {
                // 转发状态更新给原始发送者
                let mut notification = message.clone();
                notification.recipient_id = Some(original_sender_id.to_string());
                state.send_to_user(original_sender_id, &notification).await;
            } else if let Some(original_sender_id) = state.get_message_sender(message_id).await {
                // 使用记录中的发送者ID
                let mut notification = message.clone();
                notification.recipient_id = Some(original_sender_id.clone());
                state.send_to_user(&original_sender_id, &notification).await;
            }
        }
    }
}

// 处理WebRTC信令
async fn handle_webrtc_signal(message: WebSocketMessage, state: &Arc<AppState>) {
    if let Some(recipient_id) = &message.recipient_id {
        debug!("Forwarding WebRTC signal from {} to {}", message.sender_id, recipient_id);
        state.send_to_user(recipient_id, &message).await;
    }
}

// 处理输入状态指示器
async fn handle_typing_indicator(message: WebSocketMessage, state: &Arc<AppState>) {
    // 从data字段获取接收者列表
    let recipients = if let Some(data) = &message.data {
        if let Some(recipients) = data.get("recipients").and_then(|r| r.as_array()) {
            recipients.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<String>>()
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    
    // 转发给所有接收者
    for recipient_id in &recipients {
        if recipient_id != &message.sender_id {
            state.send_to_user(recipient_id, &message).await;
        }
    }
}

// 处理会话更新
async fn handle_conversation_updated(message: WebSocketMessage, state: &Arc<AppState>) {
    // 从data字段获取参与者列表
    let participants = if let Some(data) = &message.data {
        if let Some(participants) = data.get("participants").and_then(|p| p.as_array()) {
            participants.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<String>>()
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    
    // 转发给所有参与者
    for participant_id in &participants {
        if participant_id != &message.sender_id {
            state.send_to_user(participant_id, &message).await;
        }
    }
}

// 处理群组成员更新
async fn handle_group_member_update(message: WebSocketMessage, state: &Arc<AppState>) {
    // 从data字段获取参与者列表
    let participants = if let Some(data) = &message.data {
        if let Some(participants) = data.get("participants").and_then(|p| p.as_array()) {
            participants.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<String>>()
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    
    // 转发给所有参与者
    for participant_id in &participants {
        if participant_id != &message.sender_id {
            state.send_to_user(participant_id, &message).await;
        }
    }
}